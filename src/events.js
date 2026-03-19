import { getSystems } from "./query.js";
import { notifications } from "./store.js";

let notificationId = 0;

function addNotification(message, type) {
  notifications.update((list) => [{ id: notificationId++, message, type, timestamp: new Date() }, ...list]);
}

const endpoint = import.meta.env.DEV ? "http://localhost:8000/events" : `${window.location.origin}/events`;

export const datsEndpoint = import.meta.env.DEV ? "http://localhost:8000/dats" : `${window.location.origin}/dats`;

let eventSource = null;
let toastCallback = null;

export function connectSSE(onToast) {
  if (eventSource) {
    return; // Already connected
  }

  toastCallback = onToast;
  eventSource = new EventSource(endpoint);

  eventSource.addEventListener("purge_started", (event) => {
    const data = JSON.parse(event.data);
    addNotification(data.message, "info");
    if (toastCallback) {
      toastCallback(data.message, "info");
    }
  });

  eventSource.addEventListener("purge_complete", async (event) => {
    const data = JSON.parse(event.data);
    addNotification(data.message, "success");
    if (toastCallback) {
      toastCallback(data.message, "success");
    }
    await getSystems();
  });

  eventSource.addEventListener("purge_error", (event) => {
    const data = JSON.parse(event.data);
    addNotification(data.message, "error");
    if (toastCallback) {
      toastCallback(data.message, "error");
    }
  });

  eventSource.addEventListener("import_dat_started", (event) => {
    const data = JSON.parse(event.data);
    addNotification(data.message, "info");
    if (toastCallback) {
      toastCallback(data.message, "info");
    }
  });

  eventSource.addEventListener("import_dat_complete", async (event) => {
    const data = JSON.parse(event.data);
    const type = data.skipped ? "warning" : "success";
    addNotification(data.message, type);
    if (toastCallback) {
      toastCallback(data.message, type);
    }
    if (!data.skipped) {
      await getSystems();
    }
  });

  eventSource.addEventListener("import_dat_error", (event) => {
    const data = JSON.parse(event.data);
    addNotification(data.message, "error");
    if (toastCallback) {
      toastCallback(data.message, "error");
    }
  });

  eventSource.addEventListener("error", (event) => {
    console.error("SSE connection error:", event);
  });

  eventSource.addEventListener("open", () => {
    console.log("SSE connection established");
  });
}

export function disconnectSSE() {
  if (eventSource) {
    eventSource.close();
    eventSource = null;
    toastCallback = null;
  }
}

export function isSSEConnected() {
  return eventSource !== null && eventSource.readyState === EventSource.OPEN;
}
