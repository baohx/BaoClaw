/**
 * Service Worker Manager
 * 
 * 注册 Service Worker
 * 缓存静态资源
 * 检查更新
 * 
 * Requirements: 8.1
 */

export interface ServiceWorkerManager {
  register(): Promise<void>;
  getRegistration(): ServiceWorkerRegistration | null;
  checkForUpdates(): Promise<boolean>;
  applyUpdate(): void;
}

/**
 * Service Worker 管理器实现
 */
class ServiceWorkerManagerImpl implements ServiceWorkerManager {
  private registration: ServiceWorkerRegistration | null = null;
  private updateCallbacks: Set<() => void> = new Set();

  /**
   * 注册 Service Worker
   */
  async register(): Promise<void> {
    if (!('serviceWorker' in navigator)) {
      console.warn('Service Workers are not supported');
      return;
    }

    try {
      this.registration = await navigator.serviceWorker.register('/sw.js', {
        scope: '/',
      });

      console.log('Service Worker registered:', this.registration.scope);

      // Check for updates
      this.registration.addEventListener('updatefound', () => {
        const newWorker = this.registration!.installing;
        
        if (newWorker) {
          newWorker.addEventListener('statechange', () => {
            if (newWorker.state === 'installed' && navigator.serviceWorker.controller) {
              // New version available
              this.notifyUpdate();
            }
          });
        }
      });

    } catch (error) {
      console.error('Service Worker registration failed:', error);
    }
  }

  /**
   * 获取注册对象
   */
  getRegistration(): ServiceWorkerRegistration | null {
    return this.registration;
  }

  /**
   * 检查更新
   */
  async checkForUpdates(): Promise<boolean> {
    if (!this.registration) {
      return false;
    }

    try {
      await this.registration.update();
      return true;
    } catch (error) {
      console.error('Failed to check for updates:', error);
      return false;
    }
  }

  /**
   * 应用更新
   */
  applyUpdate(): void {
    if (!this.registration || !this.registration.waiting) {
      return;
    }

    // Send message to the waiting Service Worker to activate
    this.registration.waiting.postMessage({ type: 'SKIP_WAITING' });
  }

  /**
   * 注册更新回调
   */
  onUpdateAvailable(callback: () => void): void {
    this.updateCallbacks.add(callback);
  }

  /**
   * 移除更新回调
   */
  offUpdateAvailable(callback: () => void): void {
    this.updateCallbacks.delete(callback);
  }

  /**
   * 通知更新可用
   */
  private notifyUpdate(): void {
    this.updateCallbacks.forEach(callback => {
      try {
        callback();
      } catch (error) {
        console.error('Error in update callback:', error);
      }
    });
  }
}

/**
 * 单例实例
 */
let instance: ServiceWorkerManagerImpl | null = null;

/**
 * 获取 Service Worker 管理器实例
 */
export function getServiceWorkerManager(): ServiceWorkerManager {
  if (!instance) {
    instance = new ServiceWorkerManagerImpl();
  }
  return instance;
}

/**
 * 创建新的 Service Worker 管理器实例（用于测试）
 */
export function createServiceWorkerManager(): ServiceWorkerManager {
  return new ServiceWorkerManagerImpl();
}
