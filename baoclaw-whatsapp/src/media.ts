/**
 * Media handling for WhatsApp Gateway.
 * Downloads media from WhatsApp via Baileys, uploads documents to daemon,
 * sends files back to WhatsApp conversations, and detects file paths
 * in tool output.
 */
import * as fs from "fs";
import * as path from "path";
import { createLogger } from "../../ts-ipc/logger.js";

const logger = createLogger("whatsapp");
import * as os from "os";
import * as crypto from "crypto";
import { IpcClient } from "../../ts-ipc/index.js";

export interface MediaFile {
  path: string;
  mimeType: string;
  fileName: string;
  size: number;
}

const MEDIA_TMP_DIR = path.join(os.tmpdir(), "baoclaw-whatsapp");
const MAX_MEDIA_SIZE = 50 * 1024 * 1024; // 50MB
const IMAGE_EXTENSIONS = [".png", ".jpg", ".jpeg", ".webp", ".gif"];

export class MediaHandler {
  /**
   * 下载 WhatsApp 媒体文件到临时目录。
   * 使用 Baileys 的 downloadMediaMessage 辅助函数。
   */
  async downloadMedia(sock: any, msg: any): Promise<MediaFile | null> {
    // 1. 确保 tmp dir 存在
    if (!fs.existsSync(MEDIA_TMP_DIR)) {
      fs.mkdirSync(MEDIA_TMP_DIR, { recursive: true });
    }

    // 2. 使用 Baileys 的 downloadMediaMessage（从 @whiskeysockets/baileys 动态 import）
    let buffer: Buffer;
    try {
      const { downloadMediaMessage } = await import("@whiskeysockets/baileys");
      buffer = await downloadMediaMessage(msg, "buffer", {});
    } catch (err) {
      logger.error(`Failed to download media: ${err}`);
      return null;
    }

    // 3. 如果 buffer 为空或 undefined，返回 null
    if (!buffer || buffer.length === 0) {
      return null;
    }

    // 4. 检查文件大小（不超过 MAX_MEDIA_SIZE）
    if (buffer.length > MAX_MEDIA_SIZE) {
      logger.error(
        `Media file too large: ${buffer.length} bytes (max ${MAX_MEDIA_SIZE} bytes)`,
      );
      return null;
    }

    // 5. 确定文件名和 MIME 类型（从 msg.message 中提取）
    const message = msg.message ?? {};
    let mimeType = "application/octet-stream";
    let fileName = "file";

    if (message.documentMessage) {
      mimeType = message.documentMessage.mimetype ?? mimeType;
      fileName = message.documentMessage.fileName ?? fileName;
    } else if (message.imageMessage) {
      mimeType = message.imageMessage.mimetype ?? "image/jpeg";
      fileName = message.imageMessage.caption ?? "image";
    } else if (message.videoMessage) {
      mimeType = message.videoMessage.mimetype ?? "video/mp4";
      fileName = "video";
    } else if (message.audioMessage) {
      mimeType = message.audioMessage.mimetype ?? "audio/ogg";
      fileName = "audio";
    } else if (message.stickerMessage) {
      mimeType = message.stickerMessage.mimetype ?? "image/webp";
      fileName = "sticker";
    }

    // 6. 写入临时文件 /tmp/baoclaw-whatsapp/{uuid}.{ext}
    const uuid = crypto.randomUUID();
    const ext = fileName.includes(".")
      ? path.extname(fileName)
      : mimeToExtension(mimeType);
    const tmpPath = path.join(MEDIA_TMP_DIR, `${uuid}${ext}`);

    try {
      fs.writeFileSync(tmpPath, buffer);
    } catch (err) {
      logger.error(`Failed to write media temp file: ${err}`);
      return null;
    }

    // 7. 返回 MediaFile
    return {
      path: tmpPath,
      mimeType,
      fileName,
      size: buffer.length,
    };
  }

  /**
   * 处理文档消息（PDF/DOCX）。
   * 下载 → docUpload RPC → 返回文档 ID。
   */
  async handleDocument(
    sock: any,
    msg: any,
    ipcClient: IpcClient,
  ): Promise<string | null> {
    // 1. 调用 downloadMedia 下载文件
    const mediaFile = await this.downloadMedia(sock, msg);
    if (!mediaFile) {
      return null;
    }

    try {
      // 2. 调用 ipcClient.request('docUpload', { file_path: mediaFile.path })
      const result = await ipcClient.request<{
        doc_id?: string;
        document_id?: string;
        id?: string;
      }>("docUpload", { file_path: mediaFile.path });

      // 3. 返回文档 ID (result.doc_id 或类似字段)
      const docId = result?.doc_id ?? result?.document_id ?? result?.id ?? null;
      if (!docId) {
        logger.error("docUpload returned no document ID");
        return null;
      }
      return docId;
    } catch (err) {
      logger.error(`docUpload RPC error: ${err}`);
      return null;
    } finally {
      // 4. 清理临时文件
      this.cleanup(mediaFile.path);
    }
  }

  /**
   * 处理图片消息。
   * 下载 → 保存为本地文件 → 返回文件路径。
   */
  async handleImage(sock: any, msg: any): Promise<string | null> {
    // 1. 调用 downloadMedia 下载文件
    const mediaFile = await this.downloadMedia(sock, msg);
    if (!mediaFile) {
      return null;
    }

    // 2. 返回文件路径（不删除，后续可能需要发送给 daemon）
    return mediaFile.path;
  }

  /**
   * 发送文件到 WhatsApp 对话。
   * 自动判断是图片还是文档。
   */
  async sendFile(
    sock: any,
    jid: string,
    filePath: string,
    caption?: string,
  ): Promise<void> {
    // 1. 检查文件是否存在
    if (!fs.existsSync(filePath)) {
      throw new Error(`File not found: ${filePath}`);
    }

    // 2. 读取文件为 Buffer
    const buffer = fs.readFileSync(filePath);
    const ext = path.extname(filePath).toLowerCase();
    const basename = path.basename(filePath);

    // 3. 根据 extension 判断是图片还是文档
    if (isImageFile(filePath)) {
      // 4. 图片：sock.sendMessage(jid, { image: buffer, caption })
      await sock.sendMessage(jid, {
        image: buffer,
        caption: caption ?? undefined,
        mimetype: extensionToMime(ext),
      });
    } else {
      // 5. 文档：sock.sendMessage(jid, { document: buffer, fileName: basename, mimetype })
      await sock.sendMessage(jid, {
        document: buffer,
        fileName: basename,
        mimetype: extensionToMime(ext),
        caption: caption ?? undefined,
      });
    }
  }

  /**
   * 从 tool_result 输出中检测文件路径。
   * 返回检测到的文件路径列表（如果有的话）。
   */
  detectFilePaths(output: string): string[] {
    const paths: string[] = [];

    // 匹配模式 1: "bytes_written:123,file_path:/path/to/file"
    const kvPattern = /file_path:([^\s,;]+)/g;
    let match: RegExpExecArray | null;
    while ((match = kvPattern.exec(output)) !== null) {
      paths.push(match[1]);
    }

    // 匹配模式 2: "Saved to: /path/to/file"  或  "Saved to:/path/to/file"
    const savedPattern = /Saved to:\s*([^\s]+)/g;
    while ((match = savedPattern.exec(output)) !== null) {
      paths.push(match[1]);
    }

    // 匹配模式 3: 绝对路径 (以 / 开头，后跟至少一个 / 或带扩展名)
    const absPattern =
      /(?<![a-zA-Z0-9])(\/(?:home|tmp|var|etc|usr|opt|root|mnt|dev|srv)\/[^\s"'`<>,;|&()\[\]{}]+)/g;
    while ((match = absPattern.exec(output)) !== null) {
      paths.push(match[1]);
    }

    // 去重并只返回确实存在的文件
    const unique = [...new Set(paths)];
    return unique.filter((p) => {
      try {
        return fs.existsSync(p);
      } catch {
        return false;
      }
    });
  }

  /**
   * 清理临时文件。
   */
  cleanup(mediaPath: string): void {
    try {
      fs.unlinkSync(mediaPath);
    } catch {
      /* ignore */
    }
  }

  /**
   * 清理所有临时文件（shutdown 时调用）。
   */
  cleanupAll(): void {
    try {
      fs.rmSync(MEDIA_TMP_DIR, { recursive: true, force: true });
    } catch {
      /* ignore */
    }
  }
}

// ── 工具函数 ──

/** 从 MIME 类型推断文件扩展名 */
function mimeToExtension(mime: string): string {
  const map: Record<string, string> = {
    "application/pdf": ".pdf",
    "image/png": ".png",
    "image/jpeg": ".jpg",
    "image/jpg": ".jpg",
    "image/webp": ".webp",
    "image/gif": ".gif",
    "image/bmp": ".bmp",
    "image/svg+xml": ".svg",
    "video/mp4": ".mp4",
    "video/webm": ".webm",
    "video/avi": ".avi",
    "audio/ogg": ".ogg",
    "audio/mpeg": ".mp3",
    "audio/mp4": ".m4a",
    "audio/wav": ".wav",
    "audio/aac": ".aac",
    "text/plain": ".txt",
    "text/csv": ".csv",
    "text/html": ".html",
    "application/json": ".json",
    "application/zip": ".zip",
    "application/gzip": ".gz",
    "application/x-tar": ".tar",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document":
      ".docx",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet":
      ".xlsx",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation":
      ".pptx",
    "application/msword": ".doc",
    "application/vnd.ms-excel": ".xls",
    "application/vnd.ms-powerpoint": ".ppt",
  };

  // Normalize: strip parameters after ;
  const base = mime.split(";")[0].trim().toLowerCase();
  return map[base] ?? ".bin";
}

/** 从文件扩展名推断 MIME 类型 */
function extensionToMime(ext: string): string {
  const map: Record<string, string> = {
    ".png": "image/png",
    ".jpg": "image/jpeg",
    ".jpeg": "image/jpeg",
    ".webp": "image/webp",
    ".gif": "image/gif",
    ".bmp": "image/bmp",
    ".svg": "image/svg+xml",
    ".pdf": "application/pdf",
    ".doc": "application/msword",
    ".docx":
      "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ".xls": "application/vnd.ms-excel",
    ".xlsx":
      "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ".ppt": "application/vnd.ms-powerpoint",
    ".pptx":
      "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ".txt": "text/plain",
    ".csv": "text/csv",
    ".html": "text/html",
    ".json": "application/json",
    ".zip": "application/zip",
    ".gz": "application/gzip",
    ".tar": "application/x-tar",
    ".mp4": "video/mp4",
    ".webm": "video/webm",
    ".mp3": "audio/mpeg",
    ".ogg": "audio/ogg",
    ".wav": "audio/wav",
  };
  return map[ext] ?? "application/octet-stream";
}

/** 判断文件是否为图片 */
export function isImageFile(filePath: string): boolean {
  const ext = path.extname(filePath).toLowerCase();
  return IMAGE_EXTENSIONS.includes(ext);
}
