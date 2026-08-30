const CHRONOS_ENDPOINT = "http://127.0.0.1:45678/tab";

function reportActiveTab(url, title) {
  if (!url || url.startsWith("chrome://") || url.startsWith("edge://") || url.startsWith("about:") || url.startsWith("brave://")) {
    return;
  }

  fetch(CHRONOS_ENDPOINT, {
    method: "POST",
    headers: {
      "Content-Type": "application/json"
    },
    body: JSON.stringify({ url: url, title: title || "" })
  }).catch(() => {
    // Ignore errors when Chronos app is not running
  });
}

function checkCurrentActiveTab() {
  chrome.tabs.query({ active: true, lastFocusedWindow: true }, (tabs) => {
    if (tabs && tabs.length > 0 && tabs[0].url) {
      reportActiveTab(tabs[0].url, tabs[0].title);
    }
  });
}

// 1. Tab switched
chrome.tabs.onActivated.addListener((activeInfo) => {
  chrome.tabs.get(activeInfo.tabId, (tab) => {
    if (tab && tab.url) {
      reportActiveTab(tab.url, tab.title);
    }
  });
});

// 2. URL updated in current active tab
chrome.tabs.onUpdated.addListener((tabId, changeInfo, tab) => {
  if (tab.active && (changeInfo.url || changeInfo.status === "complete")) {
    reportActiveTab(tab.url, tab.title);
  }
});

// 3. Window focus changed
chrome.windows.onFocusChanged.addListener((windowId) => {
  if (windowId !== chrome.windows.WINDOW_ID_NONE) {
    checkCurrentActiveTab();
  }
});

// Initial check on worker load
checkCurrentActiveTab();
