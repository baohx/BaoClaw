/**
 * Touch Navigator Module
 * 
 * 检测左滑/右滑手势
 * 设置滑动阈值（50px）
 * 支持移动端触摸事件
 * 
 * Requirements: 4.3
 */

/**
 * 触摸导航回调函数类型
 */
export interface TouchNavigatorCallbacks {
  onNext: () => void;
  onPrev: () => void;
}

/**
 * 触摸导航器配置
 */
export interface TouchNavigatorConfig {
  /** 滑动阈值，单位像素，超过此距离才触发导航 */
  swipeThreshold?: number;
  /** 是否启用垂直滑动阻止（防止页面滚动时误触） */
  preventVerticalScroll?: boolean;
  /** 最大垂直滑动距离，超过此距离则不触发水平滑动导航 */
  maxVerticalDistance?: number;
}

/**
 * 触摸点信息
 */
interface TouchPoint {
  x: number;
  y: number;
  timestamp: number;
}

/**
 * 滑动方向
 */
type SwipeDirection = 'left' | 'right' | 'up' | 'down' | 'none';

/**
 * 触摸导航器
 * 
 * 负责处理触摸事件，检测滑动手势并触发相应的导航动作
 * 
 * 示例用法：
 * ```typescript
 * const navigator = new TouchNavigator({
 *   onNext: () => goToNextSlide(),
 *   onPrev: () => goToPrevSlide(),
 * });
 * 
 * navigator.bind(document.getElementById('slide-container')!);
 * 
 * // 使用完毕后解绑
 * navigator.unbind();
 * ```
 */
export class TouchNavigator {
  private callbacks: TouchNavigatorCallbacks;
  private config: Required<TouchNavigatorConfig>;
  private bound = false;
  private boundElement: HTMLElement | null = null;
  private touchStart: TouchPoint | null = null;
  private touchEnd: TouchPoint | null = null;
  private lastSwipe: { deltaX: number; deltaY: number; direction: SwipeDirection } | null = null;
  
  // 预绑定的事件处理函数
  private handleTouchStart: (event: TouchEvent) => void;
  private handleTouchMove: (event: TouchEvent) => void;
  private handleTouchEnd: (event: TouchEvent) => void;

  /**
   * 创建触摸导航器实例
   * 
   * @param callbacks - 导航回调函数
   * @param config - 配置选项
   */
  constructor(callbacks: TouchNavigatorCallbacks, config: TouchNavigatorConfig = {}) {
    this.callbacks = callbacks;
    this.config = {
      swipeThreshold: config.swipeThreshold ?? 50,
      preventVerticalScroll: config.preventVerticalScroll ?? false,
      maxVerticalDistance: config.maxVerticalDistance ?? 100,
    };

    // 预绑定事件处理函数
    this.handleTouchStart = this.createTouchStartHandler();
    this.handleTouchMove = this.createTouchMoveHandler();
    this.handleTouchEnd = this.createTouchEndHandler();
  }

  /**
   * 绑定触摸事件监听器到指定元素
   * 
   * @param element - 要绑定触摸事件的 HTML 元素
   */
  bind(element: HTMLElement): void {
    if (this.bound) {
      return;
    }

    this.boundElement = element;
    element.addEventListener('touchstart', this.handleTouchStart, { passive: true });
    element.addEventListener('touchmove', this.handleTouchMove, { passive: false });
    element.addEventListener('touchend', this.handleTouchEnd, { passive: true });
    this.bound = true;
  }

  /**
   * 解绑触摸事件监听器
   */
  unbind(): void {
    if (!this.bound || !this.boundElement) {
      return;
    }

    this.boundElement.removeEventListener('touchstart', this.handleTouchStart);
    this.boundElement.removeEventListener('touchmove', this.handleTouchMove);
    this.boundElement.removeEventListener('touchend', this.handleTouchEnd);
    this.bound = false;
    this.boundElement = null;
    this.touchStart = null;
    this.touchEnd = null;
  }

  /**
   * 检查是否已绑定
   */
  isBound(): boolean {
    return this.bound;
  }

  /**
   * 获取当前配置
   */
  getConfig(): Required<TouchNavigatorConfig> {
    return { ...this.config };
  }

  /**
   * 更新回调函数
   */
  updateCallbacks(callbacks: Partial<TouchNavigatorCallbacks>): void {
    this.callbacks = {
      ...this.callbacks,
      ...callbacks,
    };
  }

  /**
   * 更新配置
   */
  updateConfig(config: Partial<TouchNavigatorConfig>): void {
    this.config = {
      ...this.config,
      ...config,
    };
  }

  /**
   * 创建触摸开始事件处理函数
   */
  private createTouchStartHandler(): (event: TouchEvent) => void {
    return (event: TouchEvent) => {
      if (event.touches.length !== 1) {
        // 忽略多点触控
        this.touchStart = null;
        return;
      }

      const touch = event.touches[0];
      this.touchStart = {
        x: touch.clientX,
        y: touch.clientY,
        timestamp: Date.now(),
      };
      this.touchEnd = null;
    };
  }

  /**
   * 创建触摸移动事件处理函数
   */
  private createTouchMoveHandler(): (event: TouchEvent) => void {
    return (event: TouchEvent) => {
      if (!this.touchStart || event.touches.length !== 1) {
        return;
      }

      const touch = event.touches[0];
      this.touchEnd = {
        x: touch.clientX,
        y: touch.clientY,
        timestamp: Date.now(),
      };

      // 检测是否为水平滑动，如果是则阻止垂直滚动
      if (this.config.preventVerticalScroll) {
        const direction = this.detectSwipeDirection();
        if (direction === 'left' || direction === 'right') {
          event.preventDefault();
        }
      }
    };
  }

  /**
   * 创建触摸结束事件处理函数
   */
  private createTouchEndHandler(): (event: TouchEvent) => void {
    return (_event: TouchEvent) => {
      if (!this.touchStart || !this.touchEnd) {
        this.touchStart = null;
        this.touchEnd = null;
        return;
      }

      const direction = this.detectSwipeDirection();
      
      // 保存最后一次滑动信息
      const deltaX = this.touchEnd.x - this.touchStart.x;
      const deltaY = this.touchEnd.y - this.touchStart.y;
      this.lastSwipe = { deltaX, deltaY, direction };
      
      // 触发相应回调
      if (direction === 'left') {
        this.callbacks.onNext();
      } else if (direction === 'right') {
        this.callbacks.onPrev();
      }

      // 重置状态
      this.touchStart = null;
      this.touchEnd = null;
    };
  }

  /**
   * 检测滑动方向
   * 
   * 根据起点和终点位置判断滑动方向
   * 只有当滑动距离超过阈值且垂直距离不过大时才认为有效
   */
  private detectSwipeDirection(): SwipeDirection {
    if (!this.touchStart || !this.touchEnd) {
      return 'none';
    }

    const deltaX = this.touchEnd.x - this.touchStart.x;
    const deltaY = this.touchEnd.y - this.touchStart.y;
    const absDeltaX = Math.abs(deltaX);
    const absDeltaY = Math.abs(deltaY);

    // 检查垂直距离是否过大
    if (absDeltaY > this.config.maxVerticalDistance) {
      return 'none';
    }

    // 检查是否达到水平滑动阈值
    if (absDeltaX < this.config.swipeThreshold) {
      return 'none';
    }

    // 判断滑动方向
    if (absDeltaX > absDeltaY) {
      // 水平滑动
      return deltaX > 0 ? 'right' : 'left';
    } else {
      // 垂直滑动
      return deltaY > 0 ? 'down' : 'up';
    }
  }

  /**
   * 获取最后一次滑动的信息（用于调试）
   */
  getLastSwipeInfo(): { deltaX: number; deltaY: number; direction: SwipeDirection } | null {
    if (!this.lastSwipe) {
      return null;
    }

    return { ...this.lastSwipe };
  }
}

/**
 * 创建默认的触摸导航器
 * 
 * 工厂函数，简化创建过程
 */
export function createTouchNavigator(
  callbacks: TouchNavigatorCallbacks,
  config?: TouchNavigatorConfig
): TouchNavigator {
  return new TouchNavigator(callbacks, config);
}
