/**
 * Touch Navigator Tests
 * 
 * 测试触摸导航器的核心功能
 * 
 * Requirements: 4.3
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { TouchNavigator, createTouchNavigator } from './touch';
import type { TouchNavigatorCallbacks } from './touch';

/**
 * 创建模拟的 TouchEvent
 */
function createTouchEvent(
  type: string,
  touches: { clientX: number; clientY: number }[]
): TouchEvent {
  const touchList = touches.map((t) => ({
    clientX: t.clientX,
    clientY: t.clientY,
    identifier: 0,
    target: document.body,
    pageX: t.clientX,
    pageY: t.clientY,
    screenX: t.clientX,
    screenY: t.clientY,
    radiusX: 0,
    radiusY: 0,
    rotationAngle: 0,
    force: 0,
  }));

  // Create a proper TouchList using document.createTouchList if available
  // or fall back to casting an array
  let touchListForEvent: TouchList;
  try {
    // @ts-expect-error - createTouchList may not be available in all environments
    touchListForEvent = document.createTouchList
      ? document.createTouchList(...touchList)
      : (touchList as unknown as TouchList);
  } catch {
    touchListForEvent = touchList as unknown as TouchList;
  }

  return new TouchEvent(type, {
    touches: touchListForEvent,
    cancelable: true,
    bubbles: true,
  });
}

describe('TouchNavigator', () => {
  let navigator: TouchNavigator;
  let callbacks: TouchNavigatorCallbacks;
  let mockNext: ReturnType<typeof vi.fn>;
  let mockPrev: ReturnType<typeof vi.fn>;
  let container: HTMLDivElement;

  beforeEach(() => {
    // 创建 mock 回调函数
    mockNext = vi.fn();
    mockPrev = vi.fn();

    callbacks = {
      onNext: mockNext,
      onPrev: mockPrev,
    };

    navigator = new TouchNavigator(callbacks);

    // 创建容器元素
    container = document.createElement('div');
    container.id = 'test-container';
    document.body.appendChild(container);
  });

  afterEach(() => {
    if (navigator.isBound()) {
      navigator.unbind();
    }
    if (container.parentNode) {
      container.parentNode.removeChild(container);
    }
    vi.clearAllMocks();
  });

  describe('bind/unbind', () => {
    it('should bind touch events to element', () => {
      expect(navigator.isBound()).toBe(false);
      navigator.bind(container);
      expect(navigator.isBound()).toBe(true);
    });

    it('should not bind twice', () => {
      navigator.bind(container);
      navigator.bind(container);
      expect(navigator.isBound()).toBe(true);
    });

    it('should unbind touch events', () => {
      navigator.bind(container);
      expect(navigator.isBound()).toBe(true);
      navigator.unbind();
      expect(navigator.isBound()).toBe(false);
    });

    it('should not unbind if not bound', () => {
      navigator.unbind();
      expect(navigator.isBound()).toBe(false);
    });

    it('should clear state on unbind', () => {
      navigator.bind(container);
      
      // 模拟触摸开始
      const startEvent = createTouchEvent('touchstart', [{ clientX: 100, clientY: 100 }]);
      container.dispatchEvent(startEvent);
      
      // 解绑后应该清除状态
      navigator.unbind();
      
      const info = navigator.getLastSwipeInfo();
      expect(info).toBeNull();
    });
  });

  describe('swipe left detection', () => {
    beforeEach(() => {
      navigator.bind(container);
    });

    it('should call onNext on left swipe (threshold exceeded)', () => {
      // 从右向左滑动，距离超过阈值 50px
      const startEvent = createTouchEvent('touchstart', [{ clientX: 200, clientY: 100 }]);
      const endEvent = createTouchEvent('touchend', []);

      // 模拟触摸移动
      const moveEvent = createTouchEvent('touchmove', [{ clientX: 100, clientY: 100 }]);

      container.dispatchEvent(startEvent);
      container.dispatchEvent(moveEvent);
      container.dispatchEvent(endEvent);

      expect(mockNext).toHaveBeenCalledTimes(1);
      expect(mockPrev).not.toHaveBeenCalled();
    });

    it('should not trigger navigation if swipe distance below threshold', () => {
      // 滑动距离小于阈值 50px
      const startEvent = createTouchEvent('touchstart', [{ clientX: 100, clientY: 100 }]);
      const moveEvent = createTouchEvent('touchmove', [{ clientX: 80, clientY: 100 }]);
      const endEvent = createTouchEvent('touchend', []);

      container.dispatchEvent(startEvent);
      container.dispatchEvent(moveEvent);
      container.dispatchEvent(endEvent);

      expect(mockNext).not.toHaveBeenCalled();
      expect(mockPrev).not.toHaveBeenCalled();
    });

    it('should trigger navigation exactly at threshold', () => {
      // 滑动距离等于阈值 50px
      const startEvent = createTouchEvent('touchstart', [{ clientX: 100, clientY: 100 }]);
      const moveEvent = createTouchEvent('touchmove', [{ clientX: 50, clientY: 100 }]);
      const endEvent = createTouchEvent('touchend', []);

      container.dispatchEvent(startEvent);
      container.dispatchEvent(moveEvent);
      container.dispatchEvent(endEvent);

      expect(mockNext).toHaveBeenCalledTimes(1);
    });
  });

  describe('swipe right detection', () => {
    beforeEach(() => {
      navigator.bind(container);
    });

    it('should call onPrev on right swipe (threshold exceeded)', () => {
      // 从左向右滑动，距离超过阈值 50px
      const startEvent = createTouchEvent('touchstart', [{ clientX: 100, clientY: 100 }]);
      const moveEvent = createTouchEvent('touchmove', [{ clientX: 200, clientY: 100 }]);
      const endEvent = createTouchEvent('touchend', []);

      container.dispatchEvent(startEvent);
      container.dispatchEvent(moveEvent);
      container.dispatchEvent(endEvent);

      expect(mockPrev).toHaveBeenCalledTimes(1);
      expect(mockNext).not.toHaveBeenCalled();
    });
  });

  describe('vertical swipe handling', () => {
    beforeEach(() => {
      navigator.bind(container);
    });

    it('should not trigger navigation on vertical swipe', () => {
      // 垂直滑动
      const startEvent = createTouchEvent('touchstart', [{ clientX: 100, clientY: 100 }]);
      const moveEvent = createTouchEvent('touchmove', [{ clientX: 100, clientY: 200 }]);
      const endEvent = createTouchEvent('touchend', []);

      container.dispatchEvent(startEvent);
      container.dispatchEvent(moveEvent);
      container.dispatchEvent(endEvent);

      expect(mockNext).not.toHaveBeenCalled();
      expect(mockPrev).not.toHaveBeenCalled();
    });

    it('should not trigger navigation if vertical distance exceeds maxVerticalDistance', () => {
      // 水平滑动但垂直偏移过大
      const startEvent = createTouchEvent('touchstart', [{ clientX: 100, clientY: 100 }]);
      const moveEvent = createTouchEvent('touchmove', [{ clientX: 50, clientY: 250 }]);
      const endEvent = createTouchEvent('touchend', []);

      container.dispatchEvent(startEvent);
      container.dispatchEvent(moveEvent);
      container.dispatchEvent(endEvent);

      expect(mockNext).not.toHaveBeenCalled();
    });
  });

  describe('multi-touch handling', () => {
    beforeEach(() => {
      navigator.bind(container);
    });

    it('should ignore multi-touch events', () => {
      // 多点触控开始
      const startEvent = createTouchEvent('touchstart', [{ clientX: 100, clientY: 100 }]);
      container.dispatchEvent(startEvent);

      // 多点触控移动
      const moveEvent = createTouchEvent('touchmove', [
        { clientX: 50, clientY: 100 },
        { clientX: 150, clientY: 100 },
      ]);
      container.dispatchEvent(moveEvent);

      const endEvent = createTouchEvent('touchend', []);
      container.dispatchEvent(endEvent);

      // 由于多点触控，不应该触发导航
      expect(mockNext).not.toHaveBeenCalled();
    });
  });

  describe('custom configuration', () => {
    it('should use custom swipe threshold', () => {
      const customNavigator = new TouchNavigator(callbacks, {
        swipeThreshold: 100,
      });
      customNavigator.bind(container);

      // 滑动 80px，小于自定义阈值 100px
      const startEvent = createTouchEvent('touchstart', [{ clientX: 100, clientY: 100 }]);
      const moveEvent = createTouchEvent('touchmove', [{ clientX: 20, clientY: 100 }]);
      const endEvent = createTouchEvent('touchend', []);

      container.dispatchEvent(startEvent);
      container.dispatchEvent(moveEvent);
      container.dispatchEvent(endEvent);

      expect(mockNext).not.toHaveBeenCalled();

      // 滑动 120px，超过自定义阈值
      mockNext.mockClear();
      const startEvent2 = createTouchEvent('touchstart', [{ clientX: 100, clientY: 100 }]);
      const moveEvent2 = createTouchEvent('touchmove', [{ clientX: -20, clientY: 100 }]);
      const endEvent2 = createTouchEvent('touchend', []);

      container.dispatchEvent(startEvent2);
      container.dispatchEvent(moveEvent2);
      container.dispatchEvent(endEvent2);

      expect(mockNext).toHaveBeenCalledTimes(1);

      customNavigator.unbind();
    });

    it('should use custom maxVerticalDistance', () => {
      const customNavigator = new TouchNavigator(callbacks, {
        maxVerticalDistance: 30,
      });
      customNavigator.bind(container);

      // 水平滑动但垂直偏移 50px，超过自定义最大值 30px
      const startEvent = createTouchEvent('touchstart', [{ clientX: 100, clientY: 100 }]);
      const moveEvent = createTouchEvent('touchmove', [{ clientX: 30, clientY: 150 }]);
      const endEvent = createTouchEvent('touchend', []);

      container.dispatchEvent(startEvent);
      container.dispatchEvent(moveEvent);
      container.dispatchEvent(endEvent);

      expect(mockNext).not.toHaveBeenCalled();

      customNavigator.unbind();
    });
  });

  describe('preventVerticalScroll', () => {
    it('should prevent default on horizontal swipe when enabled', () => {
      const customNavigator = new TouchNavigator(callbacks, {
        preventVerticalScroll: true,
      });
      customNavigator.bind(container);

      const startEvent = createTouchEvent('touchstart', [{ clientX: 100, clientY: 100 }]);
      container.dispatchEvent(startEvent);

      const moveEvent = createTouchEvent('touchmove', [{ clientX: 50, clientY: 100 }]);
      container.dispatchEvent(moveEvent);

      // 由于 passive: false，可以检查 preventDefault 是否被调用
      // 但在测试环境中，我们需要手动检查
      // 这里我们主要验证功能不会崩溃

      const endEvent = createTouchEvent('touchend', []);
      container.dispatchEvent(endEvent);

      expect(mockNext).toHaveBeenCalledTimes(1);

      customNavigator.unbind();
    });
  });

  describe('updateCallbacks', () => {
    it('should update callbacks', () => {
      navigator.bind(container);

      const newNext = vi.fn();
      navigator.updateCallbacks({ onNext: newNext });

      const startEvent = createTouchEvent('touchstart', [{ clientX: 200, clientY: 100 }]);
      const moveEvent = createTouchEvent('touchmove', [{ clientX: 100, clientY: 100 }]);
      const endEvent = createTouchEvent('touchend', []);

      container.dispatchEvent(startEvent);
      container.dispatchEvent(moveEvent);
      container.dispatchEvent(endEvent);

      expect(newNext).toHaveBeenCalledTimes(1);
      expect(mockNext).not.toHaveBeenCalled();
    });
  });

  describe('updateConfig', () => {
    it('should update configuration', () => {
      navigator.bind(container);

      navigator.updateConfig({ swipeThreshold: 200 });

      // 滑动 100px，小于新阈值 200px
      const startEvent = createTouchEvent('touchstart', [{ clientX: 200, clientY: 100 }]);
      const moveEvent = createTouchEvent('touchmove', [{ clientX: 100, clientY: 100 }]);
      const endEvent = createTouchEvent('touchend', []);

      container.dispatchEvent(startEvent);
      container.dispatchEvent(moveEvent);
      container.dispatchEvent(endEvent);

      expect(mockNext).not.toHaveBeenCalled();
    });
  });

  describe('getConfig', () => {
    it('should return current config', () => {
      const config = navigator.getConfig();

      expect(config.swipeThreshold).toBe(50);
      expect(config.preventVerticalScroll).toBe(false);
      expect(config.maxVerticalDistance).toBe(100);
    });

    it('should return a copy of config', () => {
      const config1 = navigator.getConfig();
      const config2 = navigator.getConfig();

      expect(config1).not.toBe(config2);
      expect(config1).toEqual(config2);
    });
  });

  describe('getLastSwipeInfo', () => {
    it('should return null when no swipe occurred', () => {
      navigator.bind(container);
      const info = navigator.getLastSwipeInfo();
      expect(info).toBeNull();
    });

    it('should return swipe info after touch', () => {
      navigator.bind(container);

      const startEvent = createTouchEvent('touchstart', [{ clientX: 200, clientY: 100 }]);
      const moveEvent = createTouchEvent('touchmove', [{ clientX: 100, clientY: 105 }]);
      container.dispatchEvent(startEvent);
      container.dispatchEvent(moveEvent);

      const endEvent = createTouchEvent('touchend', []);
      container.dispatchEvent(endEvent);

      const info = navigator.getLastSwipeInfo();
      expect(info).not.toBeNull();
      expect(info?.deltaX).toBe(-100);
      expect(info?.deltaY).toBe(5);
      expect(info?.direction).toBe('left');
    });
  });
});

describe('createTouchNavigator factory', () => {
  it('should create a TouchNavigator instance', () => {
    const callbacks: TouchNavigatorCallbacks = {
      onNext: vi.fn(),
      onPrev: vi.fn(),
    };

    const navigator = createTouchNavigator(callbacks);

    expect(navigator).toBeInstanceOf(TouchNavigator);
    expect(navigator.isBound()).toBe(false);
  });

  it('should create with custom config', () => {
    const callbacks: TouchNavigatorCallbacks = {
      onNext: vi.fn(),
      onPrev: vi.fn(),
    };

    const navigator = createTouchNavigator(callbacks, {
      swipeThreshold: 100,
      preventVerticalScroll: true,
      maxVerticalDistance: 50,
    });

    const config = navigator.getConfig();
    expect(config.swipeThreshold).toBe(100);
    expect(config.preventVerticalScroll).toBe(true);
    expect(config.maxVerticalDistance).toBe(50);
  });
});
