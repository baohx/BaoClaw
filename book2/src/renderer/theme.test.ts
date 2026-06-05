/**
 * ThemeManager Unit Tests
 * 
 * Tests for theme switching, persistence, and CSS variable application
 * 
 * Validates: Requirements 4.6
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { ThemeManagerImpl, createThemeManager } from './theme';

describe('ThemeManager', () => {
  let themeManager: ThemeManagerImpl;
  let localStorageMock: { [key: string]: string };
  
  beforeEach(() => {
    // Reset localStorage mock
    localStorageMock = {};
    
    // Mock localStorage
    vi.stubGlobal('localStorage', {
      getItem: vi.fn((key: string) => localStorageMock[key] || null),
      setItem: vi.fn((key: string, value: string) => {
        localStorageMock[key] = value;
      }),
      removeItem: vi.fn((key: string) => {
        delete localStorageMock[key];
      }),
      clear: vi.fn(() => {
        localStorageMock = {};
      }),
    });
    
    // Mock matchMedia
    vi.stubGlobal('matchMedia', vi.fn(() => ({
      matches: false,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })));
    
    // Reset document classes
    document.documentElement.classList.remove('light', 'dark');
    
    // Create fresh instance
    themeManager = new ThemeManagerImpl();
  });
  
  afterEach(() => {
    vi.unstubAllGlobals();
    document.documentElement.classList.remove('light', 'dark');
  });
  
  describe('getTheme', () => {
    it('should return the current theme', () => {
      expect(themeManager.getTheme()).toBeDefined();
      expect(['light', 'dark']).toContain(themeManager.getTheme());
    });
    
    it('should return light theme by default when no preference is stored', () => {
      expect(themeManager.getTheme()).toBe('light');
    });
    
    it('should return stored theme preference', () => {
      localStorageMock['book2-theme'] = 'dark';
      const darkThemeManager = new ThemeManagerImpl();
      expect(darkThemeManager.getTheme()).toBe('dark');
    });
  });
  
  describe('setTheme', () => {
    it('should set the theme to dark', () => {
      themeManager.setTheme('dark');
      expect(themeManager.getTheme()).toBe('dark');
    });
    
    it('should set the theme to light', () => {
      themeManager.setTheme('dark');
      themeManager.setTheme('light');
      expect(themeManager.getTheme()).toBe('light');
    });
    
    it('should apply theme class to document root', () => {
      themeManager.setTheme('dark');
      expect(document.documentElement.classList.contains('dark')).toBe(true);
      
      themeManager.setTheme('light');
      expect(document.documentElement.classList.contains('light')).toBe(true);
    });
    
    it('should persist theme preference to localStorage', () => {
      themeManager.setTheme('dark');
      expect(localStorageMock['book2-theme']).toBe('dark');
      
      themeManager.setTheme('light');
      expect(localStorageMock['book2-theme']).toBe('light');
    });
    
    it('should not change theme if same value is set', () => {
      const callback = vi.fn();
      themeManager.onThemeChange(callback);
      
      themeManager.setTheme('light');
      expect(callback).not.toHaveBeenCalled();
    });
  });
  
  describe('toggleTheme', () => {
    it('should toggle from light to dark', () => {
      themeManager.setTheme('light');
      themeManager.toggleTheme();
      expect(themeManager.getTheme()).toBe('dark');
    });
    
    it('should toggle from dark to light', () => {
      themeManager.setTheme('dark');
      themeManager.toggleTheme();
      expect(themeManager.getTheme()).toBe('light');
    });
    
    it('should update document class when toggling', () => {
      themeManager.setTheme('light');
      themeManager.toggleTheme();
      expect(document.documentElement.classList.contains('dark')).toBe(true);
      expect(document.documentElement.classList.contains('light')).toBe(false);
    });
  });
  
  describe('onThemeChange', () => {
    it('should call callback when theme changes', () => {
      const callback = vi.fn();
      themeManager.onThemeChange(callback);
      
      themeManager.setTheme('dark');
      expect(callback).toHaveBeenCalledWith('dark');
    });
    
    it('should call multiple callbacks', () => {
      const callback1 = vi.fn();
      const callback2 = vi.fn();
      
      themeManager.onThemeChange(callback1);
      themeManager.onThemeChange(callback2);
      
      themeManager.setTheme('dark');
      expect(callback1).toHaveBeenCalledWith('dark');
      expect(callback2).toHaveBeenCalledWith('dark');
    });
    
    it('should not call callback after removal', () => {
      const callback = vi.fn();
      themeManager.onThemeChange(callback);
      
      themeManager.setTheme('dark');
      expect(callback).toHaveBeenCalledTimes(1);
      
      themeManager.offThemeChange(callback);
      themeManager.setTheme('light');
      expect(callback).toHaveBeenCalledTimes(1);
    });
  });
  
  describe('system preference', () => {
    it('should detect system dark mode preference', () => {
      vi.stubGlobal('matchMedia', vi.fn(() => ({
        matches: true, // Dark mode preferred
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      })));
      
      const systemDarkManager = new ThemeManagerImpl();
      expect(systemDarkManager.getTheme()).toBe('dark');
    });
    
    it('should detect system light mode preference', () => {
      vi.stubGlobal('matchMedia', vi.fn(() => ({
        matches: false, // Light mode preferred
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      })));
      
      const systemLightManager = new ThemeManagerImpl();
      expect(systemLightManager.getTheme()).toBe('light');
    });
  });
});

describe('createThemeManager', () => {
  it('should create a new ThemeManager instance', () => {
    const manager = createThemeManager();
    expect(manager).toBeDefined();
    expect(manager.getTheme).toBeDefined();
    expect(manager.setTheme).toBeDefined();
    expect(manager.toggleTheme).toBeDefined();
    expect(manager.onThemeChange).toBeDefined();
  });
});
