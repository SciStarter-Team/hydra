/**
 * Authentication fallback for browsers that don't support partitioned cookies
 * Primarily targeting Safari 18.5
 */

(function() {
  'use strict';

  // Check if we're in an iframe
  function isInIframe() {
    try {
      return window.self !== window.top;
    } catch (e) {
      return true;
    }
  }

  // Check if we're in a cross-site context (likely blocked cookies)
  function isCrossSite() {
    if (!isInIframe()) return false;

    try {
      // If we can access parent origin, we're same-site
      const parentOrigin = window.parent.location.origin;
      return false;
    } catch (e) {
      // Cross-origin iframe
      return true;
    }
  }

  // Detect Safari and version
  function getSafariVersion() {
    const ua = navigator.userAgent;

    // Check if it's Safari (not Chrome/Edge which also contain "Safari" in UA)
    const isSafari = /^((?!chrome|android).)*safari/i.test(ua);
    if (!isSafari) return null;

    // Extract version from "Version/XX.Y" in user agent
    const match = ua.match(/Version\/(\d+)\.(\d+)/);
    if (!match) return null;

    return {
      major: parseInt(match[1], 10),
      minor: parseInt(match[2], 10)
    };
  }

  // Check if this is Safari with broken partitioned cookie support
  function needsPartitionedCookieFallback() {
    const safariVersion = getSafariVersion();
    if (!safariVersion) return false;

    // Safari 18.5+ has broken partitioned cookie support
    // TODO: Update upper bound when Apple fixes this bug
    if (safariVersion.major === 18 && safariVersion.minor >= 5) {
      return true;
    }
    if (safariVersion.major > 18) {
      // Assume still broken in 19.x until proven otherwise
      return true;
    }

    return false;
  }

  // Request storage access (for Safari)
  async function requestStorageAccess() {
    if (!document.requestStorageAccess) {
      return false;
    }

    try {
      await document.requestStorageAccess();
      return true;
    } catch (e) {
      console.error('Storage access denied:', e);
      return false;
    }
  }

  // Open login popup
  function openLoginPopup(loginUrl, returnPath) {
    const width = 500;
    const height = 600;
    const left = (screen.width - width) / 2;
    const top = (screen.height - height) / 2;

    const popup = window.open(
      loginUrl + '?popup=1&return=' + encodeURIComponent(returnPath),
      'login',
      `width=${width},height=${height},left=${left},top=${top},toolbar=no,menubar=no`
    );

    // Listen for popup to signal login complete
    window.addEventListener('message', function handler(event) {
      // Verify origin
      if (event.origin !== window.location.origin) return;

      if (event.data.type === 'login-complete') {
        window.removeEventListener('message', handler);
        if (popup && !popup.closed) {
          popup.close();
        }
        // Reload page to pick up new auth state
        window.location.reload();
      }
    });

    return popup;
  }

  // Show auth fallback UI
  function showAuthFallback() {
    const needsAuth = document.querySelector('[data-needs-auth]');
    if (!needsAuth) return;

    const fallbackDiv = document.createElement('div');
    fallbackDiv.className = 'auth-fallback-banner';
    fallbackDiv.innerHTML = `
      <div style="background: #fff3cd; border: 1px solid #ffc107; padding: 1rem; margin: 1rem; border-radius: 4px;">
        <p><strong>Authentication Required</strong></p>
        <p>This page needs to use cookies for authentication, but they appear to be blocked in this embedded context.</p>
        <button id="request-storage-access" class="btn btn-primary">Enable Cookies</button>
        <button id="open-new-window" class="btn btn-secondary">Open in New Window</button>
      </div>
    `;

    document.body.insertBefore(fallbackDiv, document.body.firstChild);

    // Handle storage access request
    document.getElementById('request-storage-access').addEventListener('click', async () => {
      const granted = await requestStorageAccess();
      if (granted) {
        window.location.reload();
      } else {
        alert('Storage access denied. Please try opening in a new window.');
      }
    });

    // Handle open in new window
    document.getElementById('open-new-window').addEventListener('click', () => {
      window.open(window.location.href, '_blank');
    });
  }

  // Initialize on page load
  window.addEventListener('DOMContentLoaded', () => {
    // Only run checks if we're in an iframe
    if (!isInIframe()) return;

    // If cross-site and Safari with broken partitioned cookies, show fallback
    if (isCrossSite() && needsPartitionedCookieFallback()) {
      showAuthFallback();
    }
  });

  // Export functions for use in templates
  window.exchangeAuth = {
    requestStorageAccess,
    openLoginPopup,
    isInIframe,
    isCrossSite,
    getSafariVersion,
    needsPartitionedCookieFallback
  };
})();
