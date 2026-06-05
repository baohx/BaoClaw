/**
 * Section Validator Module
 * 
 * 验证章节结构完整性，确保每个章节包含所有必需部分（问题、模式、实现、思考）
 * 验证代码块源文件路径格式
 * 生成验证报告
 * 
 * Validates: Requirements 5.1-5.5
 */

import type { ParsedChapter, ValidationResult, CodeBlock } from '../types';

/**
 * 必需的章节部分名称（按顺序）
 * Requirement 5.1-5.4: 每章应包含问题、模式、实现、思考四个部分
 */
export const REQUIRED_SECTIONS = ['问题', '模式', '实现', '思考'] as const;

/**
 * 可选但推荐的章节部分名称
 * Requirement 5.5: 每章结尾应提供要点总结与延伸阅读链接
 */
export const RECOMMENDED_SECTIONS = ['总结'] as const;

/**
 * 有效的源文件路径正则表达式
 * 格式: 相对路径，指向 baoclaw-core 或其他源码目录
 * 例如: baoclaw-core/src/engine/query.rs
 */
const SOURCE_PATH_PATTERN = /^[a-zA-Z0-9_-]+(?:\/[a-zA-Z0-9_-]+)+\.[a-zA-Z0-9]+$/;

/**
 * 章节验证器接口
 */
export interface SectionValidator {
  /**
   * 验证单个章节的结构完整性
   * @param chapter 解析后的章节数据
   * @returns 验证结果
   */
  validateChapter(chapter: ParsedChapter): ValidationResult;

  /**
   * 验证所有章节的结构完整性
   * @param chapters 解析后的所有章节数据
   * @returns 所有章节的验证结果数组
   */
  validateAllChapters(chapters: ParsedChapter[]): ChapterValidationResult[];
}

/**
 * 单个章节的验证结果（包含章节标识）
 */
export interface ChapterValidationResult extends ValidationResult {
  chapterId: string;
  chapterTitle: string;
}

/**
 * 创建章节验证器实例
 */
export function createSectionValidator(): SectionValidator {
  return {
    validateChapter,
    validateAllChapters,
  };
}

/**
 * 验证单个章节的结构完整性
 * 
 * @param chapter 解析后的章节数据
 * @returns 验证结果
 */
export function validateChapter(chapter: ParsedChapter): ValidationResult {
  const missingSections: string[] = [];
  const warnings: string[] = [];

  // 验证必需部分是否存在 (Requirements 5.1-5.4)
  for (const sectionName of REQUIRED_SECTIONS) {
    const section = getSectionByKey(chapter, sectionName);
    if (!section || !section.content || section.content.trim().length === 0) {
      missingSections.push(sectionName);
    }
  }

  // 检查推荐的总结部分 (Requirement 5.5)
  const summarySection = chapter.sections.summary;
  if (!summarySection || !summarySection.content || summarySection.content.trim().length === 0) {
    warnings.push('缺少"总结"部分，建议添加要点总结与延伸阅读链接');
  }

  // 验证代码块源文件路径格式 (Requirement 3.2)
  validateCodeBlockPaths(chapter.codeBlocks, warnings);

  const valid = missingSections.length === 0;

  return {
    valid,
    missingSections,
    warnings,
  };
}

/**
 * 验证所有章节的结构完整性
 * 
 * @param chapters 解析后的所有章节数据
 * @returns 所有章节的验证结果数组
 */
export function validateAllChapters(chapters: ParsedChapter[]): ChapterValidationResult[] {
  return chapters.map((chapter) => {
    const result = validateChapter(chapter);
    return {
      ...result,
      chapterId: chapter.id,
      chapterTitle: chapter.title,
    };
  });
}

/**
 * 根据中文名称获取章节部分
 */
function getSectionByKey(
  chapter: ParsedChapter,
  sectionName: string
): { title: string; content: string; lineNumber: number } | undefined {
  const sectionMap: Record<string, keyof typeof chapter.sections> = {
    '问题': 'problem',
    '模式': 'pattern',
    '实现': 'implementation',
    '思考': 'reflection',
    '总结': 'summary',
  };

  const key = sectionMap[sectionName];
  if (!key) {
    return undefined;
  }

  return chapter.sections[key];
}

/**
 * 验证代码块源文件路径格式
 * 
 * @param codeBlocks 代码块数组
 * @param warnings 警告数组（会被修改）
 */
function validateCodeBlockPaths(codeBlocks: CodeBlock[], warnings: string[]): void {
  for (const block of codeBlocks) {
    // 只验证 Rust 和 TypeScript 代码块的源文件路径
    if (block.language === 'rust' || block.language === 'typescript') {
      if (block.sourcePath) {
        // 验证路径格式
        if (!SOURCE_PATH_PATTERN.test(block.sourcePath)) {
          warnings.push(
            `代码块 ${block.id} 的源文件路径格式无效: "${block.sourcePath}"。` +
            `应为相对路径，例如: baoclaw-core/src/engine/query.rs`
          );
        }

        // 验证路径是否以预期的源码目录开头
        const validPrefixes = ['baoclaw-core/', 'baoclaw-feishu/', 'baoclaw-telegram/'];
        const hasValidPrefix = validPrefixes.some((prefix) => block.sourcePath!.startsWith(prefix));
        
        // 对于 Rust 代码，建议使用 baoclaw-core 路径
        if (block.language === 'rust' && !hasValidPrefix && !block.sourcePath.startsWith('src/')) {
          warnings.push(
            `代码块 ${block.id} 的源文件路径可能不正确: "${block.sourcePath}"。` +
            `Rust 代码应来自 baoclaw-core 或其他项目源目录`
          );
        }
      } else {
        // Rust/TypeScript 代码块没有源文件路径标注时给出警告 (Requirement 3.2)
        warnings.push(
          `代码块 ${block.id} (${block.language}) 缺少源文件路径标注。` +
          `建议添加 path 属性指向 BaoClaw 源码位置`
        );
      }

      // 验证行号范围
      if (block.lineRange) {
        if (block.lineRange.start < 1) {
          warnings.push(
            `代码块 ${block.id} 的起始行号无效: ${block.lineRange.start}。行号应从 1 开始`
          );
        }
        if (block.lineRange.end < block.lineRange.start) {
          warnings.push(
            `代码块 ${block.id} 的行号范围无效: 起始行 ${block.lineRange.start} 大于结束行 ${block.lineRange.end}`
          );
        }
      }
    }
  }
}

/**
 * 生成验证报告的字符串表示
 * 
 * @param results 所有章节的验证结果
 * @returns 格式化的验证报告
 */
export function generateValidationReport(results: ChapterValidationResult[]): string {
  const lines: string[] = [];
  lines.push('========================================');
  lines.push('       章节结构验证报告');
  lines.push('========================================');
  lines.push('');

  const validCount = results.filter((r) => r.valid).length;
  const totalCount = results.length;

  lines.push(`总计: ${totalCount} 章，验证通过: ${validCount} 章，验证失败: ${totalCount - validCount} 章`);
  lines.push('');

  for (const result of results) {
    const status = result.valid ? '✅ 通过' : '❌ 失败';
    lines.push(`\n## ${result.chapterTitle} (${result.chapterId})`);
    lines.push(`状态: ${status}`);

    if (result.missingSections.length > 0) {
      lines.push(`缺失部分: ${result.missingSections.join(', ')}`);
    }

    if (result.warnings.length > 0) {
      lines.push('警告:');
      for (const warning of result.warnings) {
        lines.push(`  - ${warning}`);
      }
    }
  }

  lines.push('');
  lines.push('========================================');
  lines.push('验证完成');
  lines.push('========================================');

  return lines.join('\n');
}

// 默认导出
export default createSectionValidator;
