import { getSystems } from "./query.js";

const endpoint = import.meta.env.DEV ? "http://localhost:8000/events" : `${window.location.origin}/events`;

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
    if (toastCallback) {
      toastCallback(data.message, "info");
    }
  });

  eventSource.addEventListener("purge_complete", async (event) => {
    const data = JSON.parse(event.data);
    if (toastCallback) {
      toastCallback(data.message, "success");
    }
    await getSystems();
  });

  eventSource.addEventListener("purge_error", (event) => {
    const data = JSON.parse(event.data);
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
