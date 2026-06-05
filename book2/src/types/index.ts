/**
 * Type Definitions for Book2
 * 
 * 定义书籍系统的所有 TypeScript 接口和类型
 */

// ============================================================================
// Book Configuration Types
// ============================================================================

export interface BookConfig {
  title: string;
  subtitle: string;
  author: string;
  version: string;
  repository: string;
  chapters: ChapterConfig[];
  build: BuildOptions;
}

export interface ChapterConfig {
  id: string;
  path: string;
  title: string;
}

export interface BuildOptions {
  outputDir: string;
  minify: boolean;
  serviceWorker: boolean;
  syntaxTheme: SyntaxTheme;
}

export interface SyntaxTheme {
  light: string;
  dark: string;
  languages: string[];
}

// ============================================================================
// Markdown Parser Types
// ============================================================================

export interface ParsedChapter {
  id: string;
  order: number;
  title: string;
  sections: ChapterSections;
  codeBlocks: CodeBlock[];
  assets: Asset[];
  externalLinks: ExternalLink[];
}

export interface ChapterSections {
  problem?: Section;
  pattern?: Section;
  implementation?: Section;
  reflection?: Section;
  summary?: Section;
}

export interface Section {
  title: string;
  content: string;
  lineNumber: number;
}

export interface CodeBlock {
  id: string;
  language: 'rust' | 'typescript' | 'bash' | 'mermaid' | 'other';
  code: string;
  sourcePath?: string;
  lineRange?: { start: number; end: number };
}

export interface Asset {
  type: 'image' | 'diagram';
  path: string;
  alt?: string;
}

export interface ExternalLink {
  url: string;
  label: string;
  type: 'github' | 'docs' | 'reference';
}

// ============================================================================
// Validator Types
// ============================================================================

export interface ValidationResult {
  valid: boolean;
  missingSections: string[];
  warnings: string[];
}

// ============================================================================
// Slide Generator Types
// ============================================================================

export interface Slide {
  id: string;
  chapterId: string;
  chapterTitle: string;
  title: string;
  content: string;
  type: 'title' | 'problem' | 'pattern' | 'implementation' | 'reflection' | 'summary' | 'code';
  notes?: string;
  codeBlocks?: CodeBlock[];
  progress: number;
}

export interface SlideCollection {
  slides: Slide[];
  tableOfContents: TableOfContents;
  totalSlides: number;
}

export interface TableOfContents {
  chapters: ChapterEntry[];
}

export interface ChapterEntry {
  id: string;
  title: string;
  sections: string[];
  slideCount: number;
  slides: SlideEntry[];
}

export interface SlideEntry {
  id: string;
  title: string;
  type: Slide['type'];
}

// ============================================================================
// Navigation Types
// ============================================================================

export interface Position {
  chapterIndex: number;
  slideIndex: number;
  globalIndex: number;
  progress: number;
}

export interface NavigationError {
  action: 'next' | 'prev' | 'goto';
  reason: 'boundary' | 'invalid_id';
  message: string;
}

// ============================================================================
// Theme Types
// ============================================================================

export type Theme = 'light' | 'dark';

export interface ThemeManager {
  getTheme(): Theme;
  setTheme(theme: Theme): void;
  toggleTheme(): void;
  onThemeChange(callback: (theme: Theme) => void): void;
}

// ============================================================================
// Progress Types
// ============================================================================

export interface ProgressData {
  readSlides: string[];
  lastSlide: string;
  lastVisited: number;
  totalProgress: number;
}

// ============================================================================
// Error Types
// ============================================================================

export interface SlideLoadError {
  slideId: string;
  reason: 'not_found' | 'network' | 'parse_error';
  message: string;
  fallback?: Slide;
}

// ============================================================================
// Chapter Content Types
// ============================================================================

export interface Chapter {
  id: string;
  order: number;
  title: string;
  description: string;
  sections: ChapterContentSections;
  metadata: ChapterMetadata;
}

export interface ChapterMetadata {
  estimatedTime: number;
  difficulty: 'beginner' | 'intermediate' | 'advanced';
  prerequisites: string[];
  references: Reference[];
}

export interface Reference {
  title: string;
  url: string;
  type: 'github' | 'docs' | 'article';
}

export interface ChapterContentSections {
  problem: ProblemSection;
  pattern: PatternSection;
  implementation: ImplementationSection;
  reflection: ReflectionSection;
  summary: SummarySection;
}

export interface ProblemSection {
  title: string;
  description: string;
  questions: string[];
  context: string;
}

export interface PatternSection {
  title: string;
  patterns: DesignPattern[];
  diagrams?: Diagram[];
}

export interface DesignPattern {
  name: string;
  description: string;
  applicability: string;
  consequences: string[];
}

export interface Diagram {
  type: 'architecture' | 'flow' | 'sequence' | 'class';
  source: string;
  caption?: string;
}

export interface ImplementationSection {
  title: string;
  examples: CodeExample[];
  steps?: ImplementationStep[];
}

export interface CodeExample {
  id: string;
  title: string;
  description: string;
  code: string;
  language: 'rust' | 'typescript';
  sourcePath?: string;
  highlights?: number[];
  commonErrors?: ErrorExample[];
}

export interface ErrorExample {
  code: string;
  error: string;
  fix: string;
  explanation: string;
}

export interface ImplementationStep {
  step: number;
  title: string;
  description: string;
  code: string;
}

export interface ReflectionSection {
  title: string;
  alternatives: Alternative[];
  tradeoffs: Tradeoff[];
  questions: string[];
}

export interface Alternative {
  approach: string;
  pros: string[];
  cons: string[];
  when: string;
}

export interface Tradeoff {
  aspect: string;
  choice: string;
  impact: string;
}

export interface SummarySection {
  keyPoints: string[];
  furtherReading: Link[];
}

export interface Link {
  label: string;
  url: string;
}
