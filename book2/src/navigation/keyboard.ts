/**
 * Keyboard Navigator Module
 * 
 * 绑定方向键、空格键导航事件
 * 支持首页/末页快捷键
 * 支持全屏模式切换
 * 
 * Requirements: 4.2
 */

/**
 * 键盘导航回调函数类型
 */
export interface KeyboardNavigatorCallbacks {
  onNext: () => void;
  onPrev: () => void;
  onFirst: () => void;
  onLast: () => void;
  onFullscreen: () => void;
  onOverview: () => void;
}

/**
 * 键盘导航器配置
 */
export interface KeyboardNavigatorConfig {
  /** 是否启用首页/末页快捷键 */
  enableHomeEnd?: boolean;
  /** 是否启用全屏模式切换 */
  enableFullscreen?: boolean;
  /** 是否启用概览模式 */
  enableOverview?: boolean;
  /** 自定义按键映射 */
  keyMappings?: Partial<KeyMappings>;
}

/**
 * 默认按键映射
 */
interface KeyMappings {
  next: string[];
  prev: string[];
  first: string[];
  last: string[];
  fullscreen: string[];
  overview: string[];
}

const DEFAULT_KEY_MAPPINGS: KeyMappings = {
  next: ['ArrowRight', 'ArrowDown', ' ', 'Enter'],
  prev: ['ArrowLeft', 'ArrowUp'],
  first: ['Home'],
  last: ['End'],
  fullscreen: ['f', 'F'],
  overview: ['o', 'O'],
}

/**
 * 键盘导航器
 * 
 * 负责处理键盘事件，触发相应的导航动作
 */
export class KeyboardNavigator {
  private callbacks: KeyboardNavigatorCallbacks;
  private config: Required<KeyboardNavigatorConfig>;
  private keyMappings: KeyMappings;
  private bound = false;
  private handleKeyDown: (event: KeyboardEvent) => void;

  /**
   * 创建键盘导航器实例
   * 
   * @param callbacks - 导航回调函数
   * @param config - 配置选项
   */
  constructor(callbacks: KeyboardNavigatorCallbacks, config: KeyboardNavigatorConfig = {}) {
    this.callbacks = callbacks;
    this.config = {
      enableHomeEnd: config.enableHomeEnd ?? true,
      enableFullscreen: config.enableFullscreen ?? true,
      enableOverview: config.enableOverview ?? true,
      keyMappings: config.keyMappings ?? {},
    };

    // 合并默认按键映射和自定义映射
    this.keyMappings = {
      next: this.config.keyMappings.next ?? DEFAULT_KEY_MAPPINGS.next,
      prev: this.config.keyMappings.prev ?? DEFAULT_KEY_MAPPINGS.prev,
      first: this.config.keyMappings.first ?? DEFAULT_KEY_MAPPINGS.first,
      last: this.config.keyMappings.last ?? DEFAULT_KEY_MAPPINGS.last,
      fullscreen: this.config.keyMappings.fullscreen ?? DEFAULT_KEY_MAPPINGS.fullscreen,
      overview: this.config.keyMappings.overview ?? DEFAULT_KEY_MAPPINGS.overview,
    };

    // 预绑定事件处理函数
    this.handleKeyDown = this.createKeyHandler();
  }

  /**
   * 绑定键盘事件监听器
   * 
   * 注册 keydown 事件监听器到 document
   */
  bind(): void {
    if (this.bound) {
      return;
    }

    document.addEventListener('keydown', this.handleKeyDown);
    this.bound = true;
  }

  /**
   * 解绑键盘事件监听器
   * 
   * 移除 keydown 事件监听器
   */
  unbind(): void {
    if (!this.bound) {
      return;
    }

    document.removeEventListener('keydown', this.handleKeyDown);
    this.bound = false;
  }

  /**
   * 检查是否已绑定
   */
  isBound(): boolean {
    return this.bound;
  }

  /**
   * 更新回调函数
   */
  updateCallbacks(callbacks: Partial<KeyboardNavigatorCallbacks>): void {
    this.callbacks = {
      ...this.callbacks,
      ...callbacks,
    };
  }

  /**
   * 更新按键映射
   */
  updateKeyMappings(mappings: Partial<KeyMappings>): void {
    this.keyMappings = {
      ...this.keyMappings,
      ...mappings,
    };
  }

  /**
   * 创建键盘事件处理函数
   */
  private createKeyHandler(): (event: KeyboardEvent) => void {
    return (event: KeyboardEvent) => {
      // 忽略输入框内的键盘事件
      if (this.isInputElement(event.target)) {
        return;
      }

      const key = event.key;

      // 处理下一页
      if (this.keyMappings.next.includes(key)) {
        event.preventDefault();
        this.callbacks.onNext();
        return;
      }

      // 处理上一页
      if (this.keyMappings.prev.includes(key)) {
        event.preventDefault();
        this.callbacks.onPrev();
        return;
      }

      // 处理首页
      if (this.config.enableHomeEnd && this.keyMappings.first.includes(key)) {
        event.preventDefault();
        this.callbacks.onFirst();
        return;
      }

      // 处理末页
      if (this.config.enableHomeEnd && this.keyMappings.last.includes(key)) {
        event.preventDefault();
        this.callbacks.onLast();
        return;
      }

      // 处理全屏切换
      if (this.config.enableFullscreen && this.keyMappings.fullscreen.includes(key)) {
        event.preventDefault();
        this.callbacks.onFullscreen();
        return;
      }

      // 处理概览模式
      if (this.config.enableOverview && this.keyMappings.overview.includes(key)) {
        event.preventDefault();
        this.callbacks.onOverview();
        return;
      }
    };
  }

  /**
   * 检查事件目标是否为输入元素
   */
  private isInputElement(target: EventTarget | null): boolean {
    if (!target || !(target instanceof HTMLElement)) {
      return false;
    }

    const tagName = target.tagName.toLowerCase();
    const inputTypes = ['input', 'textarea', 'select'];
    
    // 检查是否为输入元素
    if (inputTypes.includes(tagName)) {
      return true;
    }

    // 检查是否为可编辑元素
    // isContentEditable 可能不是所有环境都支持，所以也检查 contenteditable 属性
    if (target.isContentEditable) {
      return true;
    }

    // 备选检查：检查 contenteditable 属性
    const contentEditable = target.getAttribute('contenteditable');
    if (contentEditable && contentEditable.toLowerCase() === 'true') {
      return true;
    }

    return false;
  }

  /**
   * 获取当前按键映射
   */
  getKeyMappings(): KeyMappings {
    return { ...this.keyMappings };
  }
}

/**
 * 创建默认的键盘导航器
 * 
 * 工厂函数，简化创建过程
 */
export function createKeyboardNavigator(
  callbacks: KeyboardNavigatorCallbacks,
  config?: KeyboardNavigatorConfig
): KeyboardNavigator {
  return new KeyboardNavigator(callbacks, config);
}
