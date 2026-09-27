import { useEffect } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import "./App.css";

const limits = [
  { id: "five-hour", title: "5h Limit" },
  { id: "weekly", title: "Weekly Limit" },
];

function App() {
  useEffect(() => {
    let active = true;
    if (isTauri()) {
      invoke<boolean>("mica_enabled")
        .then((enabled) => {
          if (active && enabled) document.documentElement.dataset.mica = "true";
        })
        .catch((error: unknown) => console.error("Unable to read window appearance:", error));
    }
    return () => {
      active = false;
      delete document.documentElement.dataset.mica;
    };
  }, []);

  return (
    <main
      className="usage-panel"
      aria-labelledby="app-title"
      onContextMenu={(event) => event.preventDefault()}
    >
      <header className="panel-header">
        <h1 id="app-title">Codex</h1>
        <div className="account-info" aria-label="Account and subscription placeholders">
          <span className="account-email" title="account@example.com">account@example.com</span>
          <span className="subscription-tier" aria-label="Subscription tier placeholder">Pro 20x</span>
        </div>
      </header>

      <div className="limits">
        {limits.map(({ id, title }) => (
          <section className="limit" key={id} aria-labelledby={id}>
            <div className="limit-heading">
              <h2 id={id}>{title}</h2>
              <span className="usage-value" aria-label="Usage unavailable">
                -
              </span>
            </div>
            <div className="usage-track" aria-hidden="true" />
            <dl className="limit-details">
              <div>
                <dt>Used</dt>
                <dd>-</dd>
              </div>
              <div>
                <dt>Resets in</dt>
                <dd>-</dd>
              </div>
            </dl>
          </section>
        ))}
      </div>

      <footer className="panel-footer">
        <span>Codex HUD</span>
      </footer>
    </main>
  );
}

export default App;
