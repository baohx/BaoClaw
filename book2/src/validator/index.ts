/**
 * Validator Module Entry Point
 * 
 * 导出所有验证器相关的接口和函数
 */

export {
  createSectionValidator,
  validateChapter,
  validateAllChapters,
  generateValidationReport,
  REQUIRED_SECTIONS,
  RECOMMENDED_SECTIONS,
  type SectionValidator,
  type ChapterValidationResult,
} from './section-validator';
