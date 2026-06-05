/**
 * URL Router Module
 * 
 * 管理页面级 URL hash
 * 支持幻灯片 URL 分享
 * 监听 hash 变化事件
 * 
 * URL 格式: #/chapter-id/slide-number
 * 例如: #/01-fundamentals/03
 * 
 * Requirements: 8.4
 */

/**
 * URL 路由器配置
 */
export interface URLRouterConfig {
  /** 是否在导航时更新 URL hash（默认 true） */
  updateHash?: boolean;
  /** 默认幻灯片 ID（当 hash 为空时使用） */
  defaultSlideId?: string;
}

/**
 * Hash 变化回调函数类型
 */
export type HashChangeCallback = (slideId: string) => void;

/**
 * 解析后的 URL 路径
 */
interface ParsedPath {
  chapterId: string;
  slideNumber: number;
}

/**
 * URL 路由器
 * 
 * 负责管理 URL hash，支持幻灯片分享和浏览器历史导航
 */
export class URLRouter {
  private config: Required<URLRouterConfig>;
  private callbacks: Set<HashChangeCallback> = new Set();
  private bound = false;
  private handleHashChange: () => void;
  private lastHash: string | null = null;

  /**
   * 创建 URL 路由器实例
   * 
   * @param config - 配置选项
   */
  constructor(config: URLRouterConfig = {}) {
    this.config = {
      updateHash: config.updateHash ?? true,
      defaultSlideId: config.defaultSlideId ?? '01-fundamentals-01',
    };

    // 预绑定事件处理函数
    this.handleHashChange = this.createHashChangeHandler();
  }

  /**
   * 获取当前幻灯片 ID
   * 
   * 从 URL hash 中解析当前幻灯片 ID
   * 
   * @returns 幻灯片 ID，如果 hash 为空则返回默认 ID
   */
  getCurrentSlideId(): string | null {
    const hash = this.getHash();
    
    if (!hash) {
      return this.config.defaultSlideId;
    }

    return this.parseHashToSlideId(hash);
  }

  /**
   * 导航到指定幻灯片
   * 
   * 更新 URL hash 以反映当前幻灯片位置
   * 
   * @param slideId - 幻灯片 ID（格式: chapter-id-slide-number）
   */
  navigateToSlide(slideId: string): void {
    if (!this.config.updateHash) {
      return;
    }

    const path = this.slideIdToPath(slideId);
    const newHash = `#/${path}`;

    // 避免重复更新相同的 hash
    if (this.lastHash === newHash) {
      return;
    }

    this.lastHash = newHash;
    
    // 更新 URL hash（会触发 hashchange 事件）
    window.location.hash = newHash;
  }

  /**
   * 静默导航到指定幻灯片
   * 
   * 更新 URL hash 但不触发 hashchange 事件回调
   * 用于在程序内部导航时避免重复触发回调
   * 
   * @param slideId - 幻灯片 ID
   */
  navigateToSlideSilent(slideId: string): void {
    if (!this.config.updateHash) {
      return;
    }

    const path = this.slideIdToPath(slideId);
    const newHash = `#/${path}`;

    if (this.lastHash === newHash) {
      return;
    }

    this.lastHash = newHash;
    
    // 使用 history API 直接修改 hash，避免触发 hashchange
    const newUrl = `${window.location.pathname}${window.location.search}${newHash}`;
    window.history.replaceState(null, '', newUrl);
  }

  /**
   * 注册 hash 变化回调
   * 
   * @param callback - hash 变化时调用的回调函数
   */
  onHashChange(callback: HashChangeCallback): void {
    this.callbacks.add(callback);
  }

  /**
   * 移除 hash 变化回调
   * 
   * @param callback - 要移除的回调函数
   */
  offHashChange(callback: HashChangeCallback): void {
    this.callbacks.delete(callback);
  }

  /**
   * 绑定事件监听器
   * 
   * 开始监听 hashchange 事件
   */
  bind(): void {
    if (this.bound) {
      return;
    }

    window.addEventListener('hashchange', this.handleHashChange);
    this.bound = true;

    // 触发初始 hash 处理
    this.handleHashChange();
  }

  /**
   * 解绑事件监听器
   * 
   * 停止监听 hashchange 事件
   */
  unbind(): void {
    if (!this.bound) {
      return;
    }

    window.removeEventListener('hashchange', this.handleHashChange);
    this.bound = false;
  }

  /**
   * 检查是否已绑定
   */
  isBound(): boolean {
    return this.bound;
  }

  /**
   * 获取当前 hash
   * 
   * @returns 当前 hash（不含 # 前缀）
   */
  private getHash(): string {
    const hash = window.location.hash;
    // 移除 # 或 #/ 前缀
    return hash.replace(/^#\/?/, '');
  }

  /**
   * 创建 hashchange 事件处理函数
   */
  private createHashChangeHandler(): () => void {
    return () => {
      const hash = this.getHash();
      const currentHash = `#/${hash}`;

      // 检查是否为我们自己设置的 hash（避免循环）
      if (this.lastHash === currentHash) {
        return;
      }

      this.lastHash = currentHash;

      // 解析幻灯片 ID
      const slideId = this.parseHashToSlideId(hash);

      // 触发所有回调
      this.callbacks.forEach(callback => {
        try {
          callback(slideId);
        } catch (error) {
          console.error('Error in hash change callback:', error);
        }
      });
    };
  }

  /**
   * 解析 hash 为幻灯片 ID
   * 
   * @param hash - URL hash（不含 # 前缀）
   * @returns 幻灯片 ID
   */
  private parseHashToSlideId(hash: string): string {
    if (!hash) {
      return this.config.defaultSlideId;
    }

    const parsed = this.parsePath(hash);
    
    if (!parsed) {
      return this.config.defaultSlideId;
    }

    // 转换为幻灯片 ID 格式: chapter-id-slide-number
    // 例如: 01-fundamentals/03 -> 01-fundamentals-03
    return `${parsed.chapterId}-${String(parsed.slideNumber).padStart(2, '0')}`;
  }

  /**
   * 解析路径字符串
   * 
   * @param path - 路径字符串（如: 01-fundamentals/03）
   * @returns 解析后的路径对象
   */
  private parsePath(path: string): ParsedPath | null {
    const parts = path.split('/');

    if (parts.length < 2) {
      return null;
    }

    const chapterId = parts[0];
    const slideNumber = parseInt(parts[1], 10);

    if (!chapterId || isNaN(slideNumber) || slideNumber < 1) {
      return null;
    }

    return { chapterId, slideNumber };
  }

  /**
   * 将幻灯片 ID 转换为 URL 路径
   * 
   * @param slideId - 幻灯片 ID（如: 01-fundamentals-03）
   * @returns URL 路径（如: 01-fundamentals/03）
   */
  private slideIdToPath(slideId: string): string {
    // 幻灯片 ID 格式: chapter-id-slide-number
    // 例如: 01-fundamentals-03
    // 需要转换为: 01-fundamentals/03

    // 查找最后一个连字符的位置
    const lastDashIndex = slideId.lastIndexOf('-');

    if (lastDashIndex === -1) {
      return slideId;
    }

    const chapterId = slideId.substring(0, lastDashIndex);
    const slideNumber = slideId.substring(lastDashIndex + 1);

    // 移除幻灯片序号的前导零（如果存在）
    const normalizedNumber = parseInt(slideNumber, 10);

    return `${chapterId}/${normalizedNumber}`;
  }

  /**
   * 获取分享链接
   * 
   * @param slideId - 幻灯片 ID（可选，默认使用当前幻灯片）
   * @returns 完整的分享 URL
   */
  getShareUrl(slideId?: string): string {
    const targetSlideId = slideId ?? this.getCurrentSlideId();
    
    if (!targetSlideId) {
      return window.location.href;
    }

    const path = this.slideIdToPath(targetSlideId);
    const baseUrl = window.location.origin + window.location.pathname;
    
    return `${baseUrl}#/${path}`;
  }

  /**
   * 重置路由器状态
   */
  reset(): void {
    this.lastHash = null;
  }
}

/**
 * 创建 URL 路由器实例
 * 
 * 工厂函数，简化创建过程
 */
export function createURLRouter(config?: URLRouterConfig): URLRouter {
  return new URLRouter(config);
}
