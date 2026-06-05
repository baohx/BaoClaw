/**
 * Keyboard Navigator Tests
 * 
 * 测试键盘导航器的核心功能
 * 
 * Requirements: 4.2
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { KeyboardNavigator, createKeyboardNavigator } from './keyboard';
import type { KeyboardNavigatorCallbacks } from './keyboard';

describe('KeyboardNavigator', () => {
  let navigator: KeyboardNavigator;
  let callbacks: KeyboardNavigatorCallbacks;
  let mockNext: ReturnType<typeof vi.fn>;
  let mockPrev: ReturnType<typeof vi.fn>;
  let mockFirst: ReturnType<typeof vi.fn>;
  let mockLast: ReturnType<typeof vi.fn>;
  let mockFullscreen: ReturnType<typeof vi.fn>;
  let mockOverview: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    // 创建 mock 回调函数
    mockNext = vi.fn();
    mockPrev = vi.fn();
    mockFirst = vi.fn();
    mockLast = vi.fn();
    mockFullscreen = vi.fn();
    mockOverview = vi.fn();

    callbacks = {
      onNext: mockNext,
      onPrev: mockPrev,
      onFirst: mockFirst,
      onLast: mockLast,
      onFullscreen: mockFullscreen,
      onOverview: mockOverview,
    };

    navigator = new KeyboardNavigator(callbacks);
  });

  afterEach(() => {
    if (navigator.isBound()) {
      navigator.unbind();
    }
    vi.clearAllMocks();
  });

  describe('bind/unbind', () => {
    it('should bind keyboard events', () => {
      expect(navigator.isBound()).toBe(false);
      navigator.bind();
      expect(navigator.isBound()).toBe(true);
    });

    it('should not bind twice', () => {
      navigator.bind();
      navigator.bind();
      expect(navigator.isBound()).toBe(true);
    });

    it('should unbind keyboard events', () => {
      navigator.bind();
      expect(navigator.isBound()).toBe(true);
      navigator.unbind();
      expect(navigator.isBound()).toBe(false);
    });

    it('should not unbind if not bound', () => {
      navigator.unbind();
      expect(navigator.isBound()).toBe(false);
    });
  });

  describe('navigation keys', () => {
    beforeEach(() => {
      navigator.bind();
    });

    it('should call onNext on ArrowRight key', () => {
      const event = new KeyboardEvent('keydown', { key: 'ArrowRight' });
      document.dispatchEvent(event);

      expect(mockNext).toHaveBeenCalledTimes(1);
    });

    it('should call onNext on Space key', () => {
      const event = new KeyboardEvent('keydown', { key: ' ' });
      document.dispatchEvent(event);

      expect(mockNext).toHaveBeenCalledTimes(1);
    });

    it('should call onNext on Enter key', () => {
      const event = new KeyboardEvent('keydown', { key: 'Enter' });
      document.dispatchEvent(event);

      expect(mockNext).toHaveBeenCalledTimes(1);
    });

    it('should call onPrev on ArrowLeft key', () => {
      const event = new KeyboardEvent('keydown', { key: 'ArrowLeft' });
      document.dispatchEvent(event);

      expect(mockPrev).toHaveBeenCalledTimes(1);
    });

    it('should prevent default behavior on navigation keys', () => {
      const event = new KeyboardEvent('keydown', { key: 'ArrowRight', cancelable: true });
      const preventDefaultSpy = vi.spyOn(event, 'preventDefault');
      
      document.dispatchEvent(event);

      expect(preventDefaultSpy).toHaveBeenCalled();
    });
  });

  describe('home/end keys', () => {
    beforeEach(() => {
      navigator.bind();
    });

    it('should call onFirst on Home key', () => {
      const event = new KeyboardEvent('keydown', { key: 'Home' });
      document.dispatchEvent(event);

      expect(mockFirst).toHaveBeenCalledTimes(1);
    });

    it('should call onLast on End key', () => {
      const event = new KeyboardEvent('keydown', { key: 'End' });
      document.dispatchEvent(event);

      expect(mockLast).toHaveBeenCalledTimes(1);
    });

    it('should not call onFirst when Home is disabled', () => {
      const newMockFirst = vi.fn();
      const customNavigator = new KeyboardNavigator({
        ...callbacks,
        onFirst: newMockFirst,
      }, {
        enableHomeEnd: false,
      });
      customNavigator.bind();

      const event = new KeyboardEvent('keydown', { key: 'Home' });
      document.dispatchEvent(event);

      expect(newMockFirst).not.toHaveBeenCalled();
      customNavigator.unbind();
    });
  });

  describe('fullscreen key', () => {
    beforeEach(() => {
      navigator.bind();
    });

    it('should call onFullscreen on "f" key', () => {
      const event = new KeyboardEvent('keydown', { key: 'f' });
      document.dispatchEvent(event);

      expect(mockFullscreen).toHaveBeenCalledTimes(1);
    });

    it('should call onFullscreen on "F" key', () => {
      const event = new KeyboardEvent('keydown', { key: 'F' });
      document.dispatchEvent(event);

      expect(mockFullscreen).toHaveBeenCalledTimes(1);
    });

    it('should not call onFullscreen when disabled', () => {
      const newMockFullscreen = vi.fn();
      const customNavigator = new KeyboardNavigator({
        ...callbacks,
        onFullscreen: newMockFullscreen,
      }, {
        enableFullscreen: false,
      });
      customNavigator.bind();

      const event = new KeyboardEvent('keydown', { key: 'f' });
      document.dispatchEvent(event);

      expect(newMockFullscreen).not.toHaveBeenCalled();
      customNavigator.unbind();
    });
  });

  describe('overview key', () => {
    beforeEach(() => {
      navigator.bind();
    });

    it('should call onOverview on "o" key', () => {
      const event = new KeyboardEvent('keydown', { key: 'o' });
      document.dispatchEvent(event);

      expect(mockOverview).toHaveBeenCalledTimes(1);
    });

    it('should call onOverview on "O" key', () => {
      const event = new KeyboardEvent('keydown', { key: 'O' });
      document.dispatchEvent(event);

      expect(mockOverview).toHaveBeenCalledTimes(1);
    });

    it('should not call onOverview when disabled', () => {
      const newMockOverview = vi.fn();
      const customNavigator = new KeyboardNavigator({
        ...callbacks,
        onOverview: newMockOverview,
      }, {
        enableOverview: false,
      });
      customNavigator.bind();

      const event = new KeyboardEvent('keydown', { key: 'o' });
      document.dispatchEvent(event);

      expect(newMockOverview).not.toHaveBeenCalled();
      customNavigator.unbind();
    });
  });

  describe('input element filtering', () => {
    beforeEach(() => {
      navigator.bind();
    });

    it('should ignore key events from input elements', () => {
      const input = document.createElement('input');
      document.body.appendChild(input);

      const event = new KeyboardEvent('keydown', { key: 'ArrowRight' });
      Object.defineProperty(event, 'target', { value: input, writable: false });
      
      document.dispatchEvent(event);

      expect(mockNext).not.toHaveBeenCalled();
      
      document.body.removeChild(input);
    });

    it('should ignore key events from textarea elements', () => {
      const textarea = document.createElement('textarea');
      document.body.appendChild(textarea);

      const event = new KeyboardEvent('keydown', { key: 'ArrowRight' });
      Object.defineProperty(event, 'target', { value: textarea, writable: false });
      
      document.dispatchEvent(event);

      expect(mockNext).not.toHaveBeenCalled();
      
      document.body.removeChild(textarea);
    });

    it('should ignore key events from contenteditable elements', () => {
      const newMockNext = vi.fn();
      const customNavigator = new KeyboardNavigator({
        ...callbacks,
        onNext: newMockNext,
      });
      customNavigator.bind();

      const div = document.createElement('div');
      div.setAttribute('contenteditable', 'true');
      document.body.appendChild(div);

      const event = new KeyboardEvent('keydown', { key: 'ArrowRight' });
      Object.defineProperty(event, 'target', { value: div, writable: false });
      
      document.dispatchEvent(event);

      expect(newMockNext).not.toHaveBeenCalled();
      
      document.body.removeChild(div);
      customNavigator.unbind();
    });
  });

  describe('custom key mappings', () => {
    it('should use custom key mappings', () => {
      const customNavigator = new KeyboardNavigator(callbacks, {
        keyMappings: {
          next: ['n', 'N'],
        },
      });
      customNavigator.bind();

      // 自定义键应该触发 next
      const customEvent = new KeyboardEvent('keydown', { key: 'n' });
      document.dispatchEvent(customEvent);
      expect(mockNext).toHaveBeenCalledTimes(1);

      // 默认键不应触发（被自定义覆盖）
      mockNext.mockClear();
      const defaultEvent = new KeyboardEvent('keydown', { key: 'ArrowRight' });
      document.dispatchEvent(defaultEvent);
      expect(mockNext).not.toHaveBeenCalled();

      customNavigator.unbind();
    });
  });

  describe('updateCallbacks', () => {
    it('should update callbacks', () => {
      navigator.bind();

      const newNext = vi.fn();
      navigator.updateCallbacks({ onNext: newNext });

      const event = new KeyboardEvent('keydown', { key: 'ArrowRight' });
      document.dispatchEvent(event);

      expect(newNext).toHaveBeenCalledTimes(1);
      expect(mockNext).not.toHaveBeenCalled();
    });
  });

  describe('updateKeyMappings', () => {
    it('should update key mappings', () => {
      navigator.bind();

      navigator.updateKeyMappings({ next: ['x'] });

      const oldEvent = new KeyboardEvent('keydown', { key: 'ArrowRight' });
      document.dispatchEvent(oldEvent);
      expect(mockNext).not.toHaveBeenCalled();

      const newEvent = new KeyboardEvent('keydown', { key: 'x' });
      document.dispatchEvent(newEvent);
      expect(mockNext).toHaveBeenCalledTimes(1);
    });
  });

  describe('getKeyMappings', () => {
    it('should return current key mappings', () => {
      const mappings = navigator.getKeyMappings();

      expect(mappings.next).toContain('ArrowRight');
      expect(mappings.prev).toContain('ArrowLeft');
      expect(mappings.first).toContain('Home');
      expect(mappings.last).toContain('End');
      expect(mappings.fullscreen).toContain('f');
      expect(mappings.overview).toContain('o');
    });

    it('should return a copy of key mappings', () => {
      const mappings1 = navigator.getKeyMappings();
      const mappings2 = navigator.getKeyMappings();

      expect(mappings1).not.toBe(mappings2);
      expect(mappings1).toEqual(mappings2);
    });
  });
});

describe('createKeyboardNavigator factory', () => {
  it('should create a KeyboardNavigator instance', () => {
    const callbacks: KeyboardNavigatorCallbacks = {
      onNext: vi.fn(),
      onPrev: vi.fn(),
      onFirst: vi.fn(),
      onLast: vi.fn(),
      onFullscreen: vi.fn(),
      onOverview: vi.fn(),
    };

    const navigator = createKeyboardNavigator(callbacks);

    expect(navigator).toBeInstanceOf(KeyboardNavigator);
    expect(navigator.isBound()).toBe(false);
  });
});
