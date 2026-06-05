/**
 * Slide Renderer Module
 * 
 * 将幻灯片数据渲染为 HTML
 * 实现幻灯片切换动画
 * 
 * Requirements: 4.1
 */

import type { Slide } from '../types';

/**
 * 动画配置
 */
interface AnimationConfig {
  duration: number;
  easing: string;
}

/**
 * 默认动画配置
 */
const DEFAULT_ANIMATION: AnimationConfig = {
  duration: 300,
  easing: 'ease-in-out',
};

/**
 * 幻灯片渲染器
 * 
 * 负责将幻灯片数据渲染到 DOM，处理动画和过渡效果
 */
export class SlideRenderer {
  private container: HTMLElement | null = null;
  private currentSlide: Slide | null = null;
  private slideElement: HTMLElement | null = null;
  private animationConfig: AnimationConfig;
  private isAnimating = false;

  constructor(animationConfig?: Partial<AnimationConfig>) {
    this.animationConfig = { ...DEFAULT_ANIMATION, ...animationConfig };
  }

  /**
   * 初始化渲染器
   * 
   * @param container - 渲染容器元素
   */
  initialize(container: HTMLElement): void {
    this.container = container;
    
    // 确保容器有相对定位（用于绝对定位的幻灯片）
    this.container.style.position = 'relative';
    this.container.style.overflow = 'hidden';
    
    // 创建幻灯片容器
    this.slideElement = document.createElement('div');
    this.slideElement.className = 'slide-container';
    this.slideElement.style.width = '100%';
    this.slideElement.style.height = '100%';
    this.container.appendChild(this.slideElement);
  }

  /**
   * 渲染幻灯片
   * 
   * @param slide - 要渲染的幻灯片数据
   * @param animate - 是否使用动画过渡（默认 true）
   */
  async render(slide: Slide, animate = true): Promise<void> {
    if (!this.container || !this.slideElement) {
      throw new Error('SlideRenderer not initialized. Call initialize() first.');
    }

    // 如果正在动画中，等待完成
    if (this.isAnimating) {
      return;
    }

    const previousSlide = this.currentSlide;
    this.currentSlide = slide;

    if (animate && previousSlide) {
      await this.animateTransition(slide);
    } else {
      this.renderSlideContent(slide);
    }
  }

  /**
   * 获取当前幻灯片
   */
  getCurrentSlide(): Slide | null {
    return this.currentSlide;
  }

  /**
   * 获取幻灯片 DOM 元素
   */
  getSlideElement(): HTMLElement {
    if (!this.slideElement) {
      throw new Error('SlideRenderer not initialized.');
    }
    return this.slideElement;
  }

  /**
   * 销毁渲染器
   */
  destroy(): void {
    if (this.slideElement && this.container) {
      this.container.removeChild(this.slideElement);
    }
    this.slideElement = null;
    this.container = null;
    this.currentSlide = null;
  }

  /**
   * 渲染幻灯片内容
   */
  private renderSlideContent(slide: Slide): void {
    if (!this.slideElement) return;

    // 清空现有内容
    this.slideElement.innerHTML = '';

    // 创建幻灯片元素
    const slideDiv = document.createElement('div');
    slideDiv.className = `slide slide-${slide.type}`;
    slideDiv.setAttribute('data-slide-id', slide.id);
    slideDiv.setAttribute('data-slide-type', slide.type);

    // 添加进度属性
    slideDiv.setAttribute('data-progress', String(slide.progress));

    // 构建幻灯片 HTML
    slideDiv.innerHTML = this.buildSlideHtml(slide);

    this.slideElement.appendChild(slideDiv);
  }

  /**
   * 构建幻灯片 HTML
   */
  private buildSlideHtml(slide: Slide): string {
    let html = '';

    // 添加幻灯片头部
    html += this.buildSlideHeader(slide);

    // 添加幻灯片主体内容
    html += `<div class="slide-body">${slide.content}</div>`;

    // 添加幻灯片底部（页码等）
    html += this.buildSlideFooter(slide);

    return html;
  }

  /**
   * 构建幻灯片头部
   */
  private buildSlideHeader(slide: Slide): string {
    let html = '<div class="slide-header">';

    // 添加章节标题（对于非标题幻灯片）
    if (slide.type !== 'title') {
      html += `<div class="slide-chapter">${this.escapeHtml(slide.chapterTitle)}</div>`;
    }

    // 添加幻灯片标题
    html += `<h2 class="slide-title">${this.escapeHtml(slide.title)}</h2>`;

    html += '</div>';
    return html;
  }

  /**
   * 构建幻灯片底部
   */
  private buildSlideFooter(slide: Slide): string {
    let html = '<div class="slide-footer">';

    // 添加进度指示
    html += `<div class="slide-progress">${slide.progress}%</div>`;

    // 添加幻灯片 ID（用于调试）
    html += `<div class="slide-id">${this.escapeHtml(slide.id)}</div>`;

    html += '</div>';
    return html;
  }

  /**
   * 执行过渡动画
   */
  private async animateTransition(slide: Slide): Promise<void> {
    if (!this.slideElement) return;

    this.isAnimating = true;

    return new Promise<void>((resolve) => {
      if (!this.slideElement) {
        this.isAnimating = false;
        resolve();
        return;
      }

      // 添加淡出动画
      this.slideElement.style.transition = `opacity ${this.animationConfig.duration}ms ${this.animationConfig.easing}`;
      this.slideElement.style.opacity = '0';

      // 等待淡出完成
      setTimeout(() => {
        // 更新内容
        this.renderSlideContent(slide);

        // 添加淡入动画
        if (this.slideElement) {
          this.slideElement.style.opacity = '1';
        }

        // 等待淡入完成
        setTimeout(() => {
          this.isAnimating = false;
          resolve();
        }, this.animationConfig.duration);
      }, this.animationConfig.duration);
    });
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
 * 创建幻灯片渲染器实例
 */
export function createSlideRenderer(animationConfig?: Partial<AnimationConfig>): SlideRenderer {
  return new SlideRenderer(animationConfig);
}
