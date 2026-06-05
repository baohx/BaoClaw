/**
 * Browser Entry Point
 * 
 * 初始化幻灯片应用，渲染所有组件
 * 
 * Requirements: 4.1
 */

import { SlideRenderer } from './renderer/slide-renderer.js';
import { getThemeManager } from './renderer/theme.js';
import { getSyntaxHighlighter } from './renderer/syntax.js';
import { KeyboardNavigator } from './navigation/keyboard.js';
import { TouchNavigator } from './navigation/touch.js';
import { URLRouter } from './navigation/router.js';
import mermaid from 'mermaid';

// 初始化 mermaid
mermaid.initialize({
  startOnLoad: false,
  theme: 'default',
  securityLevel: 'loose',
  fontFamily: 'inherit',
  flowchart: {
    useMaxWidth: true,
    htmlLabels: true,
  },
  sequence: {
    useMaxWidth: true,
  },
});

// 全局类型声明
declare global {
  interface Window {
    BOOK_DATA?: {
      slides: Slide[];
      toc: TableOfContents;
    };
  }
}

interface Slide {
  id: string;
  chapterId: string;
  chapterTitle: string;
  title: string;
  content: string;
  type: string;
  progress: number;
}

interface Chapter {
  id: string;
  title: string;
  sections: string[];
  slideCount: number;
  slides: { id: string; title: string; type: string }[];
}

interface TableOfContents {
  chapters: Chapter[];
}

class BookApp {
  private slides: Slide[] = [];
  private currentIndex = 0;
  private renderer: SlideRenderer;
  private themeManager: ReturnType<typeof getThemeManager>;
  private syntaxHighlighter: ReturnType<typeof getSyntaxHighlighter>;
  private keyboardNav: KeyboardNavigator | null = null;
  private touchNav: TouchNavigator | null = null;
  private urlRouter: URLRouter | null = null;
  private appContainer: HTMLElement;

  constructor() {
    this.appContainer = document.getElementById('app')!;
    this.renderer = new SlideRenderer();
    this.themeManager = getThemeManager();
    this.syntaxHighlighter = getSyntaxHighlighter();
  }

  async init(): Promise<void> {
    console.log('BookApp initializing...');

    // 加载幻灯片数据
    if (window.BOOK_DATA) {
      this.slides = window.BOOK_DATA.slides;
      console.log(`Loaded ${this.slides.length} slides from embedded data`);
    } else {
      // 从 slides.json 加载
      try {
        const response = await fetch('slides.json');
        const data = await response.json();
        this.slides = data;
        console.log(`Loaded ${this.slides.length} slides from slides.json`);
      } catch (error) {
        console.error('Failed to load slides:', error);
        this.showError('无法加载幻灯片数据');
        return;
      }
    }

    if (this.slides.length === 0) {
      this.showError('没有找到幻灯片');
      return;
    }

    // 初始化渲染器
    this.renderer.initialize(this.appContainer);

    // 主题管理器已在构造时自动初始化

    // 初始化键盘导航
    this.keyboardNav = new KeyboardNavigator({
      onNext: () => this.next(),
      onPrev: () => this.prev(),
      onFirst: () => this.goTo(0),
      onLast: () => this.goTo(this.slides.length - 1),
      onFullscreen: () => this.toggleFullscreen(),
      onOverview: () => this.toggleOverview(),
    });
    this.keyboardNav.bind();

    // 初始化触摸导航
    this.touchNav = new TouchNavigator(
      {
        onNext: () => this.next(),
        onPrev: () => this.prev(),
      },
      { swipeThreshold: 50 }
    );
    this.touchNav.bind(this.appContainer);

    // 初始化 URL 路由
    this.urlRouter = new URLRouter();
    this.urlRouter.bind((slideId: string) => {
      const index = this.slides.findIndex(s => s.id === slideId);
      if (index >= 0) {
        this.goTo(index, false);
      }
    });

    // 从 URL 恢复状态
    const currentSlideId = this.urlRouter.getCurrentSlideId();
    if (currentSlideId) {
      const index = this.slides.findIndex(s => s.id === currentSlideId);
      if (index >= 0) {
        this.currentIndex = index;
      }
    }

    // 渲染初始幻灯片
    this.render();

    console.log('BookApp initialized successfully');
  }

  private showError(message: string): void {
    this.appContainer.innerHTML = `
      <div class="error-message" style="padding: 2rem; text-align: center; color: #c00;">
        <h2>错误</h2>
        <p>${message}</p>
      </div>
    `;
  }

  private async render(): Promise<void> {
    const slide = this.slides[this.currentIndex];
    if (!slide) {
      console.error('No slide at index', this.currentIndex);
      return;
    }

    // 渲染幻灯片内容
    await this.renderer.render(slide);

    // 语法高亮
    this.syntaxHighlighter.highlightAll(this.appContainer);

    // 渲染 Mermaid 图表
    await this.renderMermaid();

    // 更新 URL
    if (this.urlRouter) {
      this.urlRouter.navigateToSlide(slide.id);
    }
  }

  private async renderMermaid(): Promise<void> {
    // 查找所有 mermaid 代码块
    const mermaidBlocks = this.appContainer.querySelectorAll('pre code.language-mermaid');
    
    if (mermaidBlocks.length === 0) {
      return;
    }
    
    for (const block of mermaidBlocks) {
      const code = block.textContent || '';
      const pre = block.parentElement;
      if (!pre) continue;
      
      try {
        // 清理代码（移除可能的多余空白）
        const cleanCode = code.trim();
        
        // 生成唯一 ID
        const id = `mermaid-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`;
        
        // 使用 mermaid 渲染
        const { svg } = await mermaid.render(id, cleanCode);
        
        // 创建容器并替换原来的 pre 元素
        const div = document.createElement('div');
        div.className = 'mermaid-diagram';
        div.innerHTML = svg;
        div.style.cssText = 'overflow-x: auto; padding: 1em; background: var(--bg-secondary, #f5f5f5); border-radius: 8px;';
        
        pre.replaceWith(div);
      } catch (error) {
        console.warn('Failed to render mermaid diagram:', error);
        // 保留原始代码块，但添加错误提示
        const errorDiv = document.createElement('div');
        errorDiv.className = 'mermaid-error';
        errorDiv.style.cssText = 'padding: 1em; background: #fff3f3; border: 1px solid #ffcccc; border-radius: 8px; color: #c00;';
        errorDiv.textContent = `Mermaid 图表渲染失败: ${error}`;
        pre?.replaceWith(errorDiv);
      }
    }
  }

  next(): void {
    if (this.currentIndex < this.slides.length - 1) {
      this.currentIndex++;
      this.render();
    }
  }

  prev(): void {
    if (this.currentIndex > 0) {
      this.currentIndex--;
      this.render();
    }
  }

  goTo(index: number, updateUrl = true): void {
    if (index >= 0 && index < this.slides.length && index !== this.currentIndex) {
      this.currentIndex = index;
      this.render();
    }
  }

  private toggleFullscreen(): void {
    if (!document.fullscreenElement) {
      document.documentElement.requestFullscreen().catch(err => {
        console.warn('Failed to enter fullscreen:', err);
      });
    } else {
      document.exitFullscreen();
    }
  }

  private toggleOverview(): void {
    this.appContainer.classList.toggle('overview-mode');
  }

  private toggleTheme(): void {
    this.themeManager.toggleTheme();
  }
}

// 应用初始化
document.addEventListener('DOMContentLoaded', () => {
  console.log('DOMContentLoaded - starting BookApp');
  const app = new BookApp();
  app.init().catch(err => {
    console.error('Failed to initialize BookApp:', err);
  });
});
