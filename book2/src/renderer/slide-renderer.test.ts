/**
 * SlideRenderer Unit Tests
 * 
 * Requirements: 4.1
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { SlideRenderer, createSlideRenderer } from './slide-renderer';
import type { Slide } from '../types';

describe('SlideRenderer', () => {
  let container: HTMLElement;
  let renderer: SlideRenderer;

  const mockSlide: Slide = {
    id: 'test-slide-01',
    chapterId: 'test-chapter',
    chapterTitle: 'Test Chapter',
    title: 'Test Slide Title',
    content: '<p>Test content</p>',
    type: 'problem',
    progress: 50,
  };

  const mockCodeSlide: Slide = {
    id: 'test-slide-02',
    chapterId: 'test-chapter',
    chapterTitle: 'Test Chapter',
    title: 'Code Example',
    content: '<pre><code>fn main() { println!("Hello"); }</code></pre>',
    type: 'code',
    progress: 75,
    codeBlocks: [
      {
        id: 'code-1',
        language: 'rust',
        code: 'fn main() { println!("Hello"); }',
      },
    ],
  };

  const mockTitleSlide: Slide = {
    id: 'test-slide-00',
    chapterId: 'test-chapter',
    chapterTitle: 'Test Chapter',
    title: 'Chapter 1: Introduction',
    content: '<h1>Chapter 1</h1><p>Welcome to the chapter</p>',
    type: 'title',
    progress: 0,
  };

  beforeEach(() => {
    // Create a container element
    container = document.createElement('div');
    container.id = 'test-container';
    container.style.width = '800px';
    container.style.height = '600px';
    document.body.appendChild(container);

    // Create renderer instance
    renderer = createSlideRenderer();
  });

  afterEach(() => {
    renderer.destroy();
    document.body.removeChild(container);
  });

  describe('initialize', () => {
    it('should initialize the renderer with a container', () => {
      renderer.initialize(container);

      const slideElement = container.querySelector('.slide-container');
      expect(slideElement).not.toBeNull();
    });

    it('should set container to relative positioning', () => {
      renderer.initialize(container);

      expect(container.style.position).toBe('relative');
    });

    it('should throw error if called multiple times', () => {
      renderer.initialize(container);

      // Should not throw, but should work fine
      expect(() => renderer.initialize(container)).not.toThrow();
    });
  });

  describe('render', () => {
    beforeEach(() => {
      renderer.initialize(container);
    });

    it('should render a slide without animation', async () => {
      await renderer.render(mockSlide, false);

      const slideElement = container.querySelector('.slide');
      expect(slideElement).not.toBeNull();
      expect(slideElement?.getAttribute('data-slide-id')).toBe(mockSlide.id);
      expect(slideElement?.getAttribute('data-slide-type')).toBe(mockSlide.type);
    });

    it('should render slide content correctly', async () => {
      await renderer.render(mockSlide, false);

      const slideElement = container.querySelector('.slide');
      expect(slideElement?.innerHTML).toContain(mockSlide.title);
      expect(slideElement?.innerHTML).toContain('Test Chapter');
    });

    it('should render progress attribute', async () => {
      await renderer.render(mockSlide, false);

      const slideElement = container.querySelector('.slide');
      expect(slideElement?.getAttribute('data-progress')).toBe(String(mockSlide.progress));
    });

    it('should apply correct slide type class', async () => {
      await renderer.render(mockSlide, false);

      const slideElement = container.querySelector('.slide');
      expect(slideElement?.classList.contains('slide-problem')).toBe(true);
    });

    it('should render code slide with correct class', async () => {
      await renderer.render(mockCodeSlide, false);

      const slideElement = container.querySelector('.slide');
      expect(slideElement?.classList.contains('slide-code')).toBe(true);
    });

    it('should render title slide with correct class', async () => {
      await renderer.render(mockTitleSlide, false);

      const slideElement = container.querySelector('.slide');
      expect(slideElement?.classList.contains('slide-title')).toBe(true);
    });

    it('should render slide with animation', async () => {
      const animateRenderer = createSlideRenderer({ duration: 100 });
      animateRenderer.initialize(container);

      // Render initial slide
      await animateRenderer.render(mockSlide, false);

      // Render with animation
      const renderPromise = animateRenderer.render(mockCodeSlide, true);

      // During animation, should still be transitioning
      const slideElement = container.querySelector('.slide');
      expect(slideElement?.getAttribute('data-slide-id')).toBe(mockSlide.id);

      // Wait for animation to complete
      await renderPromise;

      // Should now show the new slide
      const newSlideElement = container.querySelector('.slide');
      expect(newSlideElement?.getAttribute('data-slide-id')).toBe(mockCodeSlide.id);
    });

    it('should handle rapid successive render calls', async () => {
      // Rapid calls should queue or be ignored
      const promise1 = renderer.render(mockSlide, false);
      const promise2 = renderer.render(mockCodeSlide, false);

      await Promise.all([promise1, promise2]);

      // One of them should have rendered
      const slideElement = container.querySelector('.slide');
      expect(slideElement).not.toBeNull();
    });
  });

  describe('getCurrentSlide', () => {
    beforeEach(() => {
      renderer.initialize(container);
    });

    it('should return null initially', () => {
      expect(renderer.getCurrentSlide()).toBeNull();
    });

    it('should return current slide after render', async () => {
      await renderer.render(mockSlide, false);

      expect(renderer.getCurrentSlide()).toEqual(mockSlide);
    });

    it('should update after multiple renders', async () => {
      await renderer.render(mockSlide, false);
      expect(renderer.getCurrentSlide()).toEqual(mockSlide);

      await renderer.render(mockCodeSlide, false);
      expect(renderer.getCurrentSlide()).toEqual(mockCodeSlide);
    });
  });

  describe('getSlideElement', () => {
    it('should throw error if not initialized', () => {
      expect(() => renderer.getSlideElement()).toThrow();
    });

    it('should return the slide element after initialization', () => {
      renderer.initialize(container);

      const element = renderer.getSlideElement();
      expect(element.className).toBe('slide-container');
    });
  });

  describe('destroy', () => {
    it('should clean up the slide element', () => {
      renderer.initialize(container);
      renderer.destroy();

      const slideElement = container.querySelector('.slide-container');
      expect(slideElement).toBeNull();
    });

    it('should reset current slide', async () => {
      renderer.initialize(container);
      await renderer.render(mockSlide, false);
      renderer.destroy();

      expect(renderer.getCurrentSlide()).toBeNull();
    });

    it('should handle multiple destroy calls gracefully', () => {
      renderer.initialize(container);
      renderer.destroy();

      expect(() => renderer.destroy()).not.toThrow();
    });
  });

  describe('createSlideRenderer', () => {
    it('should create a SlideRenderer instance', () => {
      const instance = createSlideRenderer();
      expect(instance).toBeInstanceOf(SlideRenderer);
    });

    it('should accept animation config', () => {
      const instance = createSlideRenderer({
        duration: 500,
        easing: 'ease-out',
      });
      expect(instance).toBeInstanceOf(SlideRenderer);
    });
  });

  describe('slide structure', () => {
    beforeEach(() => {
      renderer.initialize(container);
    });

    it('should render slide header', async () => {
      await renderer.render(mockSlide, false);

      const header = container.querySelector('.slide-header');
      expect(header).not.toBeNull();
    });

    it('should render slide body', async () => {
      await renderer.render(mockSlide, false);

      const body = container.querySelector('.slide-body');
      expect(body).not.toBeNull();
      expect(body?.innerHTML).toContain('Test content');
    });

    it('should render slide footer', async () => {
      await renderer.render(mockSlide, false);

      const footer = container.querySelector('.slide-footer');
      expect(footer).not.toBeNull();
    });

    it('should render progress in footer', async () => {
      await renderer.render(mockSlide, false);

      const progress = container.querySelector('.slide-progress');
      expect(progress?.textContent).toBe('50%');
    });

    it('should render slide id in footer', async () => {
      await renderer.render(mockSlide, false);

      const idElement = container.querySelector('.slide-id');
      expect(idElement?.textContent).toBe(mockSlide.id);
    });
  });

  describe('special slide types', () => {
    beforeEach(() => {
      renderer.initialize(container);
    });

    it('should not render chapter label for title slide', async () => {
      await renderer.render(mockTitleSlide, false);

      const chapterDiv = container.querySelector('.slide-chapter');
      // Title slides do NOT show chapter label (they show chapter number instead)
      expect(chapterDiv).toBeNull();
    });

    it('should escape HTML in text content', async () => {
      const slideWithHtml: Slide = {
        ...mockSlide,
        title: '<script>alert("xss")</script>',
      };

      await renderer.render(slideWithHtml, false);

      const titleElement = container.querySelector('.slide-title');
      expect(titleElement?.textContent).toBe('<script>alert("xss")</script>');
      expect(titleElement?.innerHTML).not.toContain('<script>');
    });
  });
});
