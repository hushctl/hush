import { createRoot } from "react-dom/client";
import "./index.css";
import App from "./App.tsx";
import { useStore } from "./store";

// StrictMode intentionally omitted: xterm.js is an imperative DOM library that
// creates canvas elements directly — StrictMode's double-invoke of effects causes
// two terminal instances to write to the same container, producing ghost cursors
// and doubled scrollback. All other React state bugs are caught by the e2e suite.
// Force dark mode
document.documentElement.classList.add("dark");

createRoot(document.getElementById("root")!).render(<App />);

// Expose store for Playwright e2e tests (dev only)
if (import.meta.env.DEV) {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (window as any).__MC_STORE__ = useStore;
}
