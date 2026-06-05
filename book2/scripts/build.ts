/**
 * Build Script
 * 
 * 编排 Markdown 解析、验证、幻灯片生成流程
 * 输出 HTML/JS/CSS 到 dist/ 目录
 * 生成资源清单
 * 
 * Requirements: 6.3
 */

import { writeFile, mkdir, readdir, copyFile } from 'fs/promises';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';
import { MarkdownParser } from '../src/parser/markdown.js';
import { validateAllChapters, generateValidationReport } from '../src/validator/section-validator.js';
import { SlideGenerator } from '../src/generator/slide.js';
import { TOCBuilder } from '../src/generator/toc.js';

// ES module equivalent of __dirname
const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

interface BuildOptions {
  srcDir: string;
  outDir: string;
  validate: boolean;
  verbose: boolean;
}

const DEFAULT_OPTIONS: BuildOptions = {
  srcDir: join(__dirname, '..'),
  outDir: join(__dirname, '..', 'dist'),
  validate: true,
  verbose: false,
};

/**
 * 主构建函数
 */
async function build(options: Partial<BuildOptions> = {}): Promise<void> {
  const opts = { ...DEFAULT_OPTIONS, ...options };
  
  console.log('🚀 Starting book2 build...\n');

  // 1. 解析所有章节
  console.log('📖 Parsing chapters...');
  const parser = new MarkdownParser();
  const chaptersDir = join(opts.srcDir, 'chapters');
  
  let chapters: import('../src/types').ParsedChapter[];
  try {
    chapters = await parser.parseDirectory(chaptersDir);
    console.log(`   Found ${chapters.length} chapters\n`);
  } catch (error) {
    console.warn('   Warning: Could not parse chapters directory:', error);
    chapters = [];
  }

  // 2. 验证章节结构
  if (opts.validate && chapters.length > 0) {
    console.log('✅ Validating chapters...');
    const results = validateAllChapters(chapters);
    const report = generateValidationReport(results);
    console.log(report);
    
    const failedCount = results.filter(r => !r.valid).length;
    if (failedCount > 0) {
      console.warn(`\n⚠️  ${failedCount} chapter(s) have validation issues\n`);
    }
  }

  // 3. 生成幻灯片
  console.log('🎬 Generating slides...');
  const generator = new SlideGenerator();
  const collection = generator.generateAll(chapters);
  console.log(`   Generated ${collection.totalSlides} slides\n`);

  // 4. 生成目录
  console.log('📑 Building table of contents...');
  const tocBuilder = new TOCBuilder();
  const toc = tocBuilder.build(chapters);
  console.log(`   Built TOC with ${toc.chapters.length} chapters\n`);

  // 5. 创建输出目录
  console.log('📁 Creating output directory...');
  await mkdir(opts.outDir, { recursive: true });
  await mkdir(join(opts.outDir, 'assets'), { recursive: true });
  console.log(`   Output: ${opts.outDir}\n`);

  // 6. 生成 HTML
  console.log('📄 Generating HTML...');
  const html = generateHtml(collection, toc);
  await writeFile(join(opts.outDir, 'index.html'), html, 'utf-8');
  console.log('   Created index.html\n');

  // 7. 生成数据文件
  console.log('📊 Generating data files...');
  const slidesData = JSON.stringify(collection.slides, null, 2);
  await writeFile(join(opts.outDir, 'slides.json'), slidesData, 'utf-8');
  
  const tocData = JSON.stringify(toc, null, 2);
  await writeFile(join(opts.outDir, 'toc.json'), tocData, 'utf-8');
  console.log('   Created slides.json and toc.json\n');

  // 8. 复制样式文件
  console.log('🎨 Copying styles...');
  const stylesDir = join(opts.srcDir, 'styles');
  try {
    const styleFiles = await readdir(stylesDir);
    for (const file of styleFiles) {
      if (file.endsWith('.css')) {
        await copyFile(
          join(stylesDir, file),
          join(opts.outDir, file)
        );
      }
    }
    console.log(`   Copied ${styleFiles.filter(f => f.endsWith('.css')).length} CSS files\n`);
  } catch (error) {
    console.warn('   Warning: Could not copy styles:', error);
  }

  // 9. 复制资源文件
  console.log('🖼️  Copying assets...');
  const assetsDir = join(opts.srcDir, 'assets');
  try {
    await copyDirectory(assetsDir, join(opts.outDir, 'assets'));
    console.log('   Copied assets directory\n');
  } catch (error) {
    console.warn('   Warning: Could not copy assets:', error);
  }

  console.log('✨ Build complete!\n');
  console.log(`Total slides: ${collection.totalSlides}`);
  console.log(`Total chapters: ${toc.chapters.length}`);
}

/**
 * 生成 HTML 页面
 */
function generateHtml(collection: any, toc: any): string {
  return `<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <meta name="description" content="Agent Harness 实战：从 BaoClaw 看 AI Agent 系统架构">
  <meta name="theme-color" content="#ffffff">
  <title>Agent Harness 实战 - BaoClaw</title>
  
  <!-- Styles -->
  <link rel="stylesheet" href="base.css">
  <link rel="stylesheet" href="slide.css">
  <link rel="stylesheet" href="code.css">
  <link rel="stylesheet" href="print.css">
  
  <!-- PWA -->
  <link rel="manifest" href="manifest.json">
  <link rel="icon" type="image/png" href="assets/favicon.png">
</head>
<body>
  <div id="app"></div>
  
  <!-- Data -->
  <script>window.BOOK_DATA = ${JSON.stringify({ slides: collection.slides, toc })};</script>
  
  <!-- App -->
  <script type="module" src="bundle.js"></script>
</body>
</html>`;
}

/**
 * 递归复制目录
 */
async function copyDirectory(src: string, dest: string): Promise<void> {
  await mkdir(dest, { recursive: true });
  const entries = await readdir(src, { withFileTypes: true });
  
  for (const entry of entries) {
    const srcPath = join(src, entry.name);
    const destPath = join(dest, entry.name);
    
    if (entry.isDirectory()) {
      await copyDirectory(srcPath, destPath);
    } else {
      await copyFile(srcPath, destPath);
    }
  }
}

// 执行构建
build().catch(error => {
  console.error('Build failed:', error);
  process.exit(1);
});
