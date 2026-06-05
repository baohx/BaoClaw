/**
 * URL Router Unit Tests
 * 
 * 测试 URL 路由器的核心功能
 * 
 * Requirements: 8.4
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { URLRouter, createURLRouter } from './router';

describe('URLRouter', () => {
  let router: URLRouter;

  // Mock window.location
  const originalLocation = window.location;
  const originalAddEventListener = window.addEventListener;
  const originalRemoveEventListener = window.removeEventListener;

  // Track hash changes
  let hashChanges: string[] = [];
  let eventListeners: Map<string, EventListener> = new Map();

  beforeEach(() => {
    // Reset state
    hashChanges = [];
    eventListeners = new Map();

    // Mock window methods
    Object.defineProperty(window, 'location', {
      value: {
        hash: '',
        pathname: '/',
        search: '',
        origin: 'http://localhost',
        href: 'http://localhost/',
      },
      writable: true,
    });

    window.addEventListener = vi.fn((event: string, listener: EventListener) => {
      eventListeners.set(event, listener);
    });

    window.removeEventListener = vi.fn((event: string) => {
      eventListeners.delete(event);
    });

    router = new URLRouter();
  });

  afterEach(() => {
    if (router) {
      router.unbind();
    }
    vi.restoreAllMocks();
  });

  describe('getCurrentSlideId', () => {
    it('should return default slide ID when hash is empty', () => {
      (window.location as any).hash = '';
      expect(router.getCurrentSlideId()).toBe('01-fundamentals-01');
    });

    it('should parse valid hash to slide ID', () => {
      (window.location as any).hash = '#/02-core-implementation/05';
      expect(router.getCurrentSlideId()).toBe('02-core-implementation-05');
    });

    it('should handle single-digit slide numbers', () => {
      (window.location as any).hash = '#/01-fundamentals/3';
      expect(router.getCurrentSlideId()).toBe('01-fundamentals-03');
    });

    it('should return default for invalid hash format', () => {
      (window.location as any).hash = '#invalid';
      expect(router.getCurrentSlideId()).toBe('01-fundamentals-01');
    });

    it('should return default for missing slide number', () => {
      (window.location as any).hash = '#/01-fundamentals';
      expect(router.getCurrentSlideId()).toBe('01-fundamentals-01');
    });

    it('should use custom default slide ID', () => {
      const customRouter = new URLRouter({ defaultSlideId: '02-core-01' });
      (window.location as any).hash = '';
      expect(customRouter.getCurrentSlideId()).toBe('02-core-01');
    });
  });

  describe('navigateToSlide', () => {
    it('should update URL hash', () => {
      router.navigateToSlide('01-fundamentals-03');
      expect(window.location.hash).toBe('#/01-fundamentals/3');
    });

    it('should convert slide ID format correctly', () => {
      router.navigateToSlide('02-core-implementation-15');
      expect(window.location.hash).toBe('#/02-core-implementation/15');
    });

    it('should not update hash when updateHash is false', () => {
      const noUpdateRouter = new URLRouter({ updateHash: false });
      noUpdateRouter.navigateToSlide('01-fundamentals-03');
      expect(window.location.hash).toBe('');
    });

    it('should avoid duplicate hash updates', () => {
      router.navigateToSlide('01-fundamentals-03');
      const firstHash = window.location.hash;
      router.navigateToSlide('01-fundamentals-03');
      expect(window.location.hash).toBe(firstHash);
    });
  });

  describe('navigateToSlideSilent', () => {
    it('should update URL without triggering hashchange', () => {
      // Mock history API
      const replaceStateSpy = vi.spyOn(window.history, 'replaceState');

      router.navigateToSlideSilent('01-fundamentals-05');

      expect(replaceStateSpy).toHaveBeenCalled();
    });
  });

  describe('onHashChange', () => {
    it('should register callback', () => {
      const callback = vi.fn();
      router.onHashChange(callback);

      router.bind();

      // Simulate hash change
      (window.location as any).hash = '#/01-fundamentals/02';
      const hashListener = eventListeners.get('hashchange');
      if (hashListener) {
        hashListener(new Event('hashchange'));
      }

      expect(callback).toHaveBeenCalledWith('01-fundamentals-02');
    });

    it('should support multiple callbacks', () => {
      const callback1 = vi.fn();
      const callback2 = vi.fn();
      router.onHashChange(callback1);
      router.onHashChange(callback2);

      router.bind();

      (window.location as any).hash = '#/02-core/10';
      const hashListener = eventListeners.get('hashchange');
      if (hashListener) {
        hashListener(new Event('hashchange'));
      }

      expect(callback1).toHaveBeenCalled();
      expect(callback2).toHaveBeenCalled();
    });

    it('should remove callback with offHashChange', () => {
      const callback = vi.fn();
      router.onHashChange(callback);
      router.offHashChange(callback);

      router.bind();

      (window.location as any).hash = '#/01-fundamentals/02';
      const hashListener = eventListeners.get('hashchange');
      if (hashListener) {
        hashListener(new Event('hashchange'));
      }

      expect(callback).not.toHaveBeenCalled();
    });
  });

  describe('bind/unbind', () => {
    it('should add event listener on bind', () => {
      router.bind();
      expect(eventListeners.has('hashchange')).toBe(true);
    });

    it('should not bind twice', () => {
      router.bind();
      router.bind();
      expect(eventListeners.size).toBe(1);
    });

    it('should remove event listener on unbind', () => {
      router.bind();
      router.unbind();
      expect(router.isBound()).toBe(false);
    });
  });

  describe('getShareUrl', () => {
    it('should generate share URL for current slide', () => {
      (window.location as any).pathname = '/book/';
      (window.location as any).origin = 'https://example.com';
      (window.location as any).hash = '#/03-memory/07';

      const url = router.getShareUrl();
      expect(url).toBe('https://example.com/book/#/03-memory/7');
    });

    it('should generate share URL for specific slide', () => {
      (window.location as any).pathname = '/book/';
      (window.location as any).origin = 'https://example.com';

      const url = router.getShareUrl('02-core-15');
      expect(url).toBe('https://example.com/book/#/02-core/15');
    });
  });

  describe('reset', () => {
    it('should reset lastHash', () => {
      router.navigateToSlide('01-fundamentals-03');
      router.reset();
      // After reset, navigating to same slide should work
      router.navigateToSlide('01-fundamentals-03');
      expect(window.location.hash).toBe('#/01-fundamentals/3');
    });
  });
});

describe('createURLRouter', () => {
  it('should create URLRouter instance', () => {
    const router = createURLRouter();
    expect(router).toBeInstanceOf(URLRouter);
  });

  it('should pass config to instance', () => {
    // Reset location hash
    (window.location as any).hash = '';
    const router = createURLRouter({ defaultSlideId: 'custom-01' });
    expect(router.getCurrentSlideId()).toBe('custom-01');
  });
});

describe('URL Format Conversion', () => {
  let router: URLRouter;

  beforeEach(() => {
    router = new URLRouter();
  });

  afterEach(() => {
    router.unbind();
  });

  describe('slide ID to path conversion', () => {
    it('should handle various slide ID formats', () => {
      const testCases = [
        { slideId: '01-fundamentals-01', expectedPath: '/01-fundamentals/1' },
        { slideId: '02-core-implementation-15', expectedPath: '/02-core-implementation/15' },
        { slideId: '06-advanced-patterns-03', expectedPath: '/06-advanced-patterns/3' },
      ];

      testCases.forEach(({ slideId, expectedPath }) => {
        router.navigateToSlide(slideId);
        expect(window.location.hash).toBe(`#${expectedPath}`);
      });
    });
  });

  describe('path to slide ID conversion', () => {
    it('should handle various hash formats', () => {
      const testCases = [
        { hash: '#/01-fundamentals/1', expectedId: '01-fundamentals-01' },
        { hash: '#/02-core-implementation/15', expectedId: '02-core-implementation-15' },
        { hash: '#/03-memory-context/5', expectedId: '03-memory-context-05' },
      ];

      testCases.forEach(({ hash, expectedId }) => {
        (window.location as any).hash = hash;
        expect(router.getCurrentSlideId()).toBe(expectedId);
      });
    });
  });
});


/**
 * Property-Based Tests for URL Router
 * 
 * Property 6: URL Hash Controls Slide Display
 * For any valid slide URL hash (e.g., #/01-fundamentals/03), the renderer SHALL display the corresponding slide.
 * 
 * Validates: Requirements 8.4
 */
import * as fc from 'fast-check';

describe('URL Router Properties', () => {
  let router: URLRouter;
  let eventListeners: Map<string, EventListener> = new Map();

  beforeEach(() => {
    eventListeners = new Map();

    Object.defineProperty(window, 'location', {
      value: {
        hash: '',
        pathname: '/',
        search: '',
        origin: 'http://localhost',
        href: 'http://localhost/',
      },
      writable: true,
    });

    window.addEventListener = vi.fn((event: string, listener: EventListener) => {
      eventListeners.set(event, listener);
    });

    window.removeEventListener = vi.fn((event: string) => {
      eventListeners.delete(event);
    });

    router = new URLRouter();
  });

  afterEach(() => {
    if (router) {
      router.unbind();
    }
  });

  describe('Property 6: URL Hash Controls Slide Display', () => {
    it('should parse any valid URL hash format to a slide ID', () => {
      fc.assert(
        fc.property(
          // Generate valid chapter IDs (like "01-fundamentals")
          fc.tuple(
            fc.integer({ min: 1, max: 99 }),
            fc.stringMatching(/^[a-z]+(-[a-z]+)*$/)
          ).map(([num, name]) => `${String(num).padStart(2, '0')}-${name}`),
          // Generate valid slide numbers
          fc.integer({ min: 1, max: 99 }),
          (chapterId, slideNumber) => {
            const hash = `#/${chapterId}/${slideNumber}`;
            (window.location as any).hash = hash;

            const slideId = router.getCurrentSlideId();
            const expectedId = `${chapterId}-${String(slideNumber).padStart(2, '0')}`;

            return slideId === expectedId;
          }
        ),
        { numRuns: 100 }
      );
    });

    it('should generate valid URL hash for any slide ID', () => {
      fc.assert(
        fc.property(
          fc.tuple(
            fc.integer({ min: 1, max: 99 }),
            fc.stringMatching(/^[a-z]+(-[a-z]+)*$/)
          ).map(([num, name]) => `${String(num).padStart(2, '0')}-${name}`),
          fc.integer({ min: 1, max: 99 }),
          (chapterId, slideNumber) => {
            const slideId = `${chapterId}-${String(slideNumber).padStart(2, '0')}`;
            router.navigateToSlide(slideId);

            const expectedHash = `#/${chapterId}/${slideNumber}`;
            return window.location.hash === expectedHash;
          }
        ),
        { numRuns: 100 }
      );
    });

    it('should maintain roundtrip consistency between slide ID and URL hash', () => {
      fc.assert(
        fc.property(
          fc.tuple(
            fc.integer({ min: 1, max: 99 }),
            fc.stringMatching(/^[a-z]+(-[a-z]+)*$/)
          ).map(([num, name]) => `${String(num).padStart(2, '0')}-${name}`),
          fc.integer({ min: 1, max: 99 }),
          (chapterId, slideNumber) => {
            const originalSlideId = `${chapterId}-${String(slideNumber).padStart(2, '0')}`;

            // Navigate to slide (slide ID -> URL hash)
            router.navigateToSlide(originalSlideId);

            // Parse the hash back to slide ID
            const parsedSlideId = router.getCurrentSlideId();

            return parsedSlideId === originalSlideId;
          }
        ),
        { numRuns: 100 }
      );
    });
  });
});
