/**
 * Service Worker for BaoClaw Book
 * 
 * Implements cache-first strategy for offline support
 * 
 * Requirements: 8.1
 */

const CACHE_NAME = 'book2-v1';
const STATIC_CACHE = 'book2-static-v1';

// Resources to cache immediately
const PRECACHE_URLS = [
  '/',
  '/index.html',
  '/styles/base.css',
  '/styles/slide.css',
  '/styles/code.css',
  '/styles/print.css',
  '/manifest.json',
];

// Install event - precache static resources
self.addEventListener('install', (event) => {
  console.log('[ServiceWorker] Install');
  
  event.waitUntil(
    caches.open(STATIC_CACHE)
      .then((cache) => {
        console.log('[ServiceWorker] Precaching static resources');
        return cache.addAll(PRECACHE_URLS);
      })
      .then(() => {
        // Activate immediately
        return self.skipWaiting();
      })
  );
});

// Activate event - clean up old caches
self.addEventListener('activate', (event) => {
  console.log('[ServiceWorker] Activate');
  
  event.waitUntil(
    caches.keys().then((cacheNames) => {
      return Promise.all(
        cacheNames
          .filter((cacheName) => {
            // Delete old caches
            return cacheName.startsWith('book2-') && 
                   cacheName !== CACHE_NAME && 
                   cacheName !== STATIC_CACHE;
          })
          .map((cacheName) => {
            console.log('[ServiceWorker] Deleting old cache:', cacheName);
            return caches.delete(cacheName);
          })
      );
    }).then(() => {
      // Take control immediately
      return self.clients.claim();
    })
  );
});

// Fetch event - cache-first strategy
self.addEventListener('fetch', (event) => {
  const { request } = event;
  const url = new URL(request.url);

  // Only handle same-origin requests
  if (url.origin !== location.origin) {
    return;
  }

  // Skip non-GET requests
  if (request.method !== 'GET') {
    return;
  }

  event.respondWith(
    caches.match(request).then((cachedResponse) => {
      // Return cached response if available
      if (cachedResponse) {
        // Update cache in background (stale-while-revalidate)
        event.waitUntil(
          fetch(request).then((networkResponse) => {
            if (networkResponse && networkResponse.status === 200) {
              const cache = caches.open(CACHE_NAME);
              cache.then((c) => c.put(request, networkResponse));
            }
          }).catch(() => {
            // Network request failed, but we have cached version
          })
        );
        
        return cachedResponse;
      }

      // No cache, fetch from network
      return fetch(request).then((networkResponse) => {
        // Don't cache if not a valid response
        if (!networkResponse || networkResponse.status !== 200 || networkResponse.type !== 'basic') {
          return networkResponse;
        }

        // Clone the response
        const responseToCache = networkResponse.clone();

        // Cache the fetched resource
        caches.open(CACHE_NAME).then((cache) => {
          cache.put(request, responseToCache);
        });

        return networkResponse;
      }).catch(() => {
        // Network failed and no cache
        // Return offline fallback for HTML pages
        if (request.headers.get('accept').includes('text/html')) {
          return caches.match('/index.html');
        }
        
        // Return nothing for other resources
        return new Response('', { status: 503, statusText: 'Service Unavailable' });
      });
    })
  );
});

// Message event - handle update requests
self.addEventListener('message', (event) => {
  if (event.data && event.data.type === 'SKIP_WAITING') {
    self.skipWaiting();
  }
});

console.log('[ServiceWorker] Loaded');
