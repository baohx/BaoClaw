/**
 * Controls Component
 * 
 * 渲染导航按钮
 * 渲染主题切换按钮
 * 
 * Requirements: 4.2, 4.6
 */

export interface ControlsCallbacks {
  onNext: () => void;
  onPrev: () => void;
  onToggleTheme: () => void;
  onToggleSidebar?: () => void;
  onFullscreen?: () => void;
}

/**
 * 控制组件
 */
export class Controls {
  private container: HTMLElement | null = null;
  private callbacks: ControlsCallbacks;
  private isDarkTheme: boolean = false;

  constructor(callbacks: ControlsCallbacks) {
    this.callbacks = callbacks;
  }

  /**
   * 渲染控制按钮
   */
  render(): HTMLElement {
    this.container = document.createElement('div');
    this.container.className = 'controls';
    this.container.innerHTML = this.buildHtml();

    this.bindEvents();

    return this.container;
  }

  /**
   * 构建 HTML
   */
  private buildHtml(): string {
    return `
      <div class="controls-left">
        <button class="control-btn sidebar-btn" aria-label="Toggle sidebar" title="Toggle sidebar">
          <span class="icon">☰</span>
        </button>
      </div>
      
      <div class="controls-center">
        <button class="control-btn prev-btn" aria-label="Previous slide" title="Previous (←)">
          <span class="icon">◀</span>
        </button>
        <button class="control-btn next-btn" aria-label="Next slide" title="Next (→)">
          <span class="icon">▶</span>
        </button>
      </div>
      
      <div class="controls-right">
        <button class="control-btn fullscreen-btn" aria-label="Fullscreen" title="Fullscreen (f)">
          <span class="icon">⛶</span>
        </button>
        <button class="control-btn theme-btn" aria-label="Toggle theme" title="Toggle theme">
          <span class="icon theme-icon">${this.isDarkTheme ? '☀️' : '🌙'}</span>
        </button>
      </div>
    `;
  }

  /**
   * 绑定事件
   */
  private bindEvents(): void {
    if (!this.container) return;

    // 上一页按钮
    const prevBtn = this.container.querySelector('.prev-btn');
    prevBtn?.addEventListener('click', () => this.callbacks.onPrev());

    // 下一页按钮
    const nextBtn = this.container.querySelector('.next-btn');
    nextBtn?.addEventListener('click', () => this.callbacks.onNext());

    // 主题切换按钮
    const themeBtn = this.container.querySelector('.theme-btn');
    themeBtn?.addEventListener('click', () => {
      this.isDarkTheme = !this.isDarkTheme;
      this.updateThemeIcon();
      this.callbacks.onToggleTheme();
    });

    // 侧边栏按钮
    const sidebarBtn = this.container.querySelector('.sidebar-btn');
    sidebarBtn?.addEventListener('click', () => this.callbacks.onToggleSidebar?.());

    // 全屏按钮
    const fullscreenBtn = this.container.querySelector('.fullscreen-btn');
    fullscreenBtn?.addEventListener('click', () => this.callbacks.onFullscreen?.());
  }

  /**
   * 更新主题图标
   */
  private updateThemeIcon(): void {
    const themeIcon = this.container?.querySelector('.theme-icon');
    if (themeIcon) {
      themeIcon.textContent = this.isDarkTheme ? '☀️' : '🌙';
    }
  }

  /**
   * 设置主题状态
   */
  setTheme(isDark: boolean): void {
    this.isDarkTheme = isDark;
    this.updateThemeIcon();
  }

  /**
   * 更新导航按钮状态
   */
  updateNavigation(canGoPrev: boolean, canGoNext: boolean): void {
    const prevBtn = this.container?.querySelector('.prev-btn');
    const nextBtn = this.container?.querySelector('.next-btn');

    if (prevBtn) {
      (prevBtn as HTMLButtonElement).disabled = !canGoPrev;
    }

    if (nextBtn) {
      (nextBtn as HTMLButtonElement).disabled = !canGoNext;
    }
  }
}

/**
 * 创建控制组件实例
 */
export function createControls(callbacks: ControlsCallbacks): Controls {
  return new Controls(callbacks);
}
