/**
 * Sidebar Component
 * 
 * 渲染章节目录
 * 高亮当前幻灯片
 * 支持折叠/展开
 * 
 * Requirements: 4.4
 */

import type { TableOfContents, ChapterEntry } from '../types';

export interface SidebarOptions {
  collapsed?: boolean;
  showSlideCount?: boolean;
}

/**
 * 侧边栏组件
 */
export class Sidebar {
  private container: HTMLElement | null = null;
  private toc: TableOfContents | null = null;
  private currentSlideId: string | null = null;
  private collapsed: boolean;
  private showSlideCount: boolean;
  private callbacks: Set<(slideId: string) => void> = new Set();

  constructor(options: SidebarOptions = {}) {
    this.collapsed = options.collapsed ?? false;
    this.showSlideCount = options.showSlideCount ?? true;
  }

  /**
   * 渲染侧边栏
   */
  render(toc: TableOfContents): HTMLElement {
    this.toc = toc;
    
    this.container = document.createElement('aside');
    this.container.className = `sidebar${this.collapsed ? ' collapsed' : ''}`;
    this.container.innerHTML = this.buildHtml(toc);

    this.bindEvents();

    return this.container;
  }

  /**
   * 构建 HTML
   */
  private buildHtml(toc: TableOfContents): string {
    let html = `
      <div class="sidebar-header">
        <h2 class="sidebar-title">目录</h2>
        <button class="sidebar-toggle" aria-label="Toggle sidebar">
          <span class="toggle-icon">${this.collapsed ? '☰' : '✕'}</span>
        </button>
      </div>
      <nav class="sidebar-nav">
        <ul class="chapter-list">
    `;

    for (const chapter of toc.chapters) {
      html += this.buildChapterHtml(chapter);
    }

    html += `
        </ul>
      </nav>
    `;

    return html;
  }

  /**
   * 构建章节 HTML
   */
  private buildChapterHtml(chapter: ChapterEntry): string {
    const slideCount = this.showSlideCount ? `<span class="slide-count">${chapter.slideCount}</span>` : '';
    
    let html = `
      <li class="chapter-item" data-chapter-id="${chapter.id}">
        <div class="chapter-header">
          <a href="#/${chapter.id}" class="chapter-link">
            <span class="chapter-title">${this.escapeHtml(chapter.title)}</span>
            ${slideCount}
          </a>
          <button class="chapter-toggle" aria-label="Expand chapter">
            <span class="toggle-icon">▶</span>
          </button>
        </div>
        <ul class="section-list">
    `;

    for (const section of chapter.sections) {
      html += `
        <li class="section-item">
          <span class="section-name">${this.escapeHtml(section)}</span>
        </li>
      `;
    }

    html += `
        </ul>
      </li>
    `;

    return html;
  }

  /**
   * 绑定事件
   */
  private bindEvents(): void {
    if (!this.container) return;

    // 折叠/展开按钮
    const toggleBtn = this.container.querySelector('.sidebar-toggle');
    toggleBtn?.addEventListener('click', () => this.toggle());

    // 章节折叠按钮
    const chapterToggles = this.container.querySelectorAll('.chapter-toggle');
    chapterToggles.forEach(btn => {
      btn.addEventListener('click', (e) => {
        e.preventDefault();
        const chapterItem = (e.currentTarget as HTMLElement).closest('.chapter-item');
        chapterItem?.classList.toggle('expanded');
      });
    });

    // 导航链接
    const links = this.container.querySelectorAll('.chapter-link');
    links.forEach(link => {
      link.addEventListener('click', (e) => {
        e.preventDefault();
        const href = (e.currentTarget as HTMLAnchorElement).getAttribute('href');
        if (href) {
          const slideId = href.replace('#/', '');
          this.navigateTo(slideId);
        }
      });
    });
  }

  /**
   * 高亮当前幻灯片
   */
  highlightCurrentSlide(slideId: string): void {
    if (!this.container) return;

    // 移除之前的高亮
    this.container.querySelectorAll('.active').forEach(el => {
      el.classList.remove('active');
    });

    // 添加新的高亮
    this.currentSlideId = slideId;
    
    // 找到对应的章节
    const chapterId = slideId.split('-').slice(0, 2).join('-');
    const chapterItem = this.container.querySelector(`[data-chapter-id="${chapterId}"]`);
    
    if (chapterItem) {
      chapterItem.classList.add('active');
      chapterItem.classList.add('expanded');
    }
  }

  /**
   * 导航到幻灯片
   */
  private navigateTo(slideId: string): void {
    this.callbacks.forEach(callback => {
      try {
        callback(slideId);
      } catch (error) {
        console.error('Error in sidebar navigation callback:', error);
      }
    });
  }

  /**
   * 注册导航回调
   */
  onNavigate(callback: (slideId: string) => void): void {
    this.callbacks.add(callback);
  }

  /**
   * 移除导航回调
   */
  offNavigate(callback: (slideId: string) => void): void {
    this.callbacks.delete(callback);
  }

  /**
   * 切换折叠状态
   */
  toggle(): void {
    this.collapsed = !this.collapsed;
    this.container?.classList.toggle('collapsed', this.collapsed);
    
    const toggleIcon = this.container?.querySelector('.sidebar-toggle .toggle-icon');
    if (toggleIcon) {
      toggleIcon.textContent = this.collapsed ? '☰' : '✕';
    }
  }

  /**
   * 检查是否折叠
   */
  isCollapsed(): boolean {
    return this.collapsed;
  }

  /**
   * HTML 转义
   */
  private escapeHtml(text: string): string {
    const escapeMap: Record<string, string> = {
      '&': '&amp;',
      '<': '&lt;',
      '>': '&gt;',
      '"': '&quot;',
      "'": '&#39;',
    };
    return text.replace(/[&<>"']/g, char => escapeMap[char]);
  }
}

/**
 * 创建侧边栏实例
 */
export function createSidebar(options?: SidebarOptions): Sidebar {
  return new Sidebar(options);
}
