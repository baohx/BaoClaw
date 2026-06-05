/**
 * Theme Manager
 * 
 * Manages theme switching between light and dark modes.
 * Persists user preference in localStorage and applies CSS variable themes.
 * 
 * Validates: Requirements 4.6
 */

import type { Theme, ThemeManager as IThemeManager } from '../types';

/**
 * Storage key for persisting theme preference
 */
const THEME_STORAGE_KEY = 'book2-theme';

/**
 * ThemeManager implementation
 * 
 * Supports:
 * - Dark/light theme switching
 * - Persistence of user preference in localStorage
 * - CSS variable theme application via document root class
 * - System preference detection (prefers-color-scheme)
 * - Event callbacks for theme changes
 */
export class ThemeManagerImpl implements IThemeManager {
  private currentTheme: Theme;
  private callbacks: Set<(theme: Theme) => void> = new Set();
  
  constructor() {
    // Initialize theme from storage or system preference
    this.currentTheme = this.loadTheme();
    this.applyTheme(this.currentTheme);
    
    // Listen for system preference changes
    this.watchSystemPreference();
  }
  
  /**
   * Get the current theme
   */
  getTheme(): Theme {
    return this.currentTheme;
  }
  
  /**
   * Set the theme
   * @param theme - The theme to set ('light' or 'dark')
   */
  setTheme(theme: Theme): void {
    if (this.currentTheme === theme) {
      return;
    }
    
    this.currentTheme = theme;
    this.applyTheme(theme);
    this.saveTheme(theme);
    this.notifyCallbacks(theme);
  }
  
  /**
   * Toggle between light and dark themes
   */
  toggleTheme(): void {
    const newTheme: Theme = this.currentTheme === 'light' ? 'dark' : 'light';
    this.setTheme(newTheme);
  }
  
  /**
   * Register a callback for theme changes
   * @param callback - Function to call when theme changes
   */
  onThemeChange(callback: (theme: Theme) => void): void {
    this.callbacks.add(callback);
  }
  
  /**
   * Remove a theme change callback
   * @param callback - Function to remove
   */
  offThemeChange(callback: (theme: Theme) => void): void {
    this.callbacks.delete(callback);
  }
  
  /**
   * Apply theme to document root element
   * @param theme - Theme to apply
   */
  private applyTheme(theme: Theme): void {
    const root = document.documentElement;
    
    // Remove existing theme classes
    root.classList.remove('light', 'dark');
    
    // Add new theme class
    root.classList.add(theme);
    
    // Update meta theme-color for mobile browsers
    this.updateMetaThemeColor(theme);
  }
  
  /**
   * Load theme from localStorage or detect system preference
   */
  private loadTheme(): Theme {
    // Check localStorage first
    const stored = localStorage.getItem(THEME_STORAGE_KEY);
    if (stored === 'light' || stored === 'dark') {
      return stored;
    }
    
    // Fall back to system preference
    return this.getSystemPreference();
  }
  
  /**
   * Save theme preference to localStorage
   * @param theme - Theme to save
   */
  private saveTheme(theme: Theme): void {
    try {
      localStorage.setItem(THEME_STORAGE_KEY, theme);
    } catch (error) {
      // localStorage may be unavailable in some environments
      console.warn('Failed to save theme preference:', error);
    }
  }
  
  /**
   * Get the system's preferred color scheme
   */
  private getSystemPreference(): Theme {
    if (typeof window !== 'undefined' && window.matchMedia) {
      return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
    }
    return 'light';
  }
  
  /**
   * Watch for system preference changes
   */
  private watchSystemPreference(): void {
    if (typeof window === 'undefined' || !window.matchMedia) {
      return;
    }
    
    const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
    
    mediaQuery.addEventListener('change', (e) => {
      // Only auto-switch if user hasn't explicitly set a preference
      const stored = localStorage.getItem(THEME_STORAGE_KEY);
      if (!stored) {
        const newTheme: Theme = e.matches ? 'dark' : 'light';
        this.setTheme(newTheme);
      }
    });
  }
  
  /**
   * Update meta theme-color for mobile browsers
   * @param theme - Current theme
   */
  private updateMetaThemeColor(theme: Theme): void {
    const metaThemeColor = document.querySelector('meta[name="theme-color"]');
    if (metaThemeColor) {
      metaThemeColor.setAttribute('content', theme === 'dark' ? '#0d1117' : '#ffffff');
    }
  }
  
  /**
   * Notify all registered callbacks of theme change
   * @param theme - New theme
   */
  private notifyCallbacks(theme: Theme): void {
    this.callbacks.forEach(callback => {
      try {
        callback(theme);
      } catch (error) {
        console.error('Error in theme change callback:', error);
      }
    });
  }
}

/**
 * Singleton instance of ThemeManager
 */
let instance: ThemeManagerImpl | null = null;

/**
 * Get the ThemeManager singleton instance
 */
export function getThemeManager(): IThemeManager {
  if (!instance) {
    instance = new ThemeManagerImpl();
  }
  return instance;
}

/**
 * Create a new ThemeManager instance (useful for testing)
 */
export function createThemeManager(): IThemeManager {
  return new ThemeManagerImpl();
}

export default ThemeManagerImpl;
