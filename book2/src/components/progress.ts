/**
 * Progress Tracker Component
 * 
 * 显示阅读进度条
 * 持久化阅读进度到 localStorage
 * 
 * Requirements: 8.5
 */

export interface ProgressData {
  readSlides: string[];
  lastSlide: string;
  lastVisited: number;
  totalProgress: number;
}

const STORAGE_KEY = 'book2-progress';

/**
 * 进度追踪器
 */
export class ProgressTracker {
  private data: ProgressData;
  private callbacks: Set<(progress: number) => void> = new Set();

  constructor() {
    this.data = this.loadProgress();
  }

  /**
   * 获取当前进度（0-100）
   */
  getCurrentProgress(): number {
    return this.data.totalProgress;
  }

  /**
   * 标记幻灯片为已读
   */
  markAsRead(slideId: string): void {
    if (!this.data.readSlides.includes(slideId)) {
      this.data.readSlides.push(slideId);
    }
    this.data.lastSlide = slideId;
    this.data.lastVisited = Date.now();
    this.saveProgress();
  }

  /**
   * 更新总进度
   */
  updateProgress(current: number, total: number): void {
    this.data.totalProgress = Math.round((current / total) * 100);
    this.saveProgress();
    this.notifyCallbacks();
  }

  /**
   * 获取已读幻灯片列表
   */
  getReadSlides(): string[] {
    return [...this.data.readSlides];
  }

  /**
   * 获取最后访问的幻灯片
   */
  getLastSlide(): string | null {
    return this.data.lastSlide || null;
  }

  /**
   * 保存进度到 localStorage
   */
  saveProgress(): void {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(this.data));
    } catch (error) {
      console.warn('Failed to save progress:', error);
    }
  }

  /**
   * 从 localStorage 加载进度
   */
  loadProgress(): ProgressData {
    try {
      const stored = localStorage.getItem(STORAGE_KEY);
      if (stored) {
        return JSON.parse(stored);
      }
    } catch (error) {
      console.warn('Failed to load progress:', error);
    }

    return {
      readSlides: [],
      lastSlide: '',
      lastVisited: 0,
      totalProgress: 0,
    };
  }

  /**
   * 重置进度
   */
  reset(): void {
    this.data = {
      readSlides: [],
      lastSlide: '',
      lastVisited: 0,
      totalProgress: 0,
    };
    this.saveProgress();
    this.notifyCallbacks();
  }

  /**
   * 注册进度变化回调
   */
  onProgressChange(callback: (progress: number) => void): void {
    this.callbacks.add(callback);
  }

  /**
   * 移除进度变化回调
   */
  offProgressChange(callback: (progress: number) => void): void {
    this.callbacks.delete(callback);
  }

  /**
   * 通知所有回调
   */
  private notifyCallbacks(): void {
    this.callbacks.forEach(callback => {
      try {
        callback(this.data.totalProgress);
      } catch (error) {
        console.error('Error in progress callback:', error);
      }
    });
  }

  /**
   * 渲染进度条 HTML
   */
  render(): string {
    return `
      <div class="progress-tracker">
        <div class="progress-bar">
          <div class="progress-fill" style="width: ${this.data.totalProgress}%"></div>
        </div>
        <div class="progress-text">${this.data.totalProgress}%</div>
      </div>
    `;
  }

  /**
   * 更新 DOM 中的进度显示
   */
  updateDOM(): void {
    const fillElement = document.querySelector('.progress-fill');
    const textElement = document.querySelector('.progress-text');
    
    if (fillElement) {
      (fillElement as HTMLElement).style.width = `${this.data.totalProgress}%`;
    }
    
    if (textElement) {
      textElement.textContent = `${this.data.totalProgress}%`;
    }
  }
}

/**
 * 创建进度追踪器实例
 */
export function createProgressTracker(): ProgressTracker {
  return new ProgressTracker();
}
