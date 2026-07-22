// Injected into every browser-pane webview. Exposes window.__miraRun for
// the automation bridge. Results leave the page by navigating to a
// mira-result:// URL that the Rust side intercepts and cancels — the page
// never gets IPC access to the app.
(function () {
  if (window.__miraInit) return;
  window.__miraInit = true;
  let nextId = 0;

  function visible(el) {
    const r = el.getBoundingClientRect();
    if (r.width === 0 && r.height === 0) return false;
    const style = getComputedStyle(el);
    return style.visibility !== "hidden" && style.display !== "none";
  }

  function textOf(el) {
    const t =
      el.getAttribute("aria-label") ||
      el.innerText ||
      el.value ||
      el.alt ||
      el.placeholder ||
      "";
    return String(t).replace(/\s+/g, " ").trim().slice(0, 100);
  }

  function collect() {
    const selector = [
      "a[href]",
      "button",
      "input",
      "select",
      "textarea",
      "[role=button]",
      "[role=link]",
      "[role=checkbox]",
      "[role=tab]",
      "[role=menuitem]",
      "[onclick]",
      "h1",
      "h2",
      "h3",
      "label",
      "img[alt]",
    ].join(",");
    const nodes = [];
    for (const el of document.querySelectorAll(selector)) {
      if (!visible(el)) continue;
      if (!el.dataset.miraId) el.dataset.miraId = String(++nextId);
      const entry = {
        id: Number(el.dataset.miraId),
        tag: el.tagName.toLowerCase(),
        text: textOf(el),
      };
      if (el.type) entry.type = el.type;
      if (el.href) entry.href = el.href;
      if (el.tagName === "INPUT" || el.tagName === "TEXTAREA")
        entry.value = String(el.value).slice(0, 100);
      if (el.checked !== undefined) entry.checked = el.checked;
      if (el.disabled) entry.disabled = true;
      nodes.push(entry);
      if (nodes.length >= 400) break;
    }
    return nodes;
  }

  function locate(ref) {
    if (/^\d+$/.test(String(ref)))
      return document.querySelector(`[data-mira-id="${ref}"]`);
    try {
      return document.querySelector(ref);
    } catch {
      return null;
    }
  }

  function setNativeValue(el, value) {
    const proto =
      el.tagName === "TEXTAREA"
        ? HTMLTextAreaElement.prototype
        : HTMLInputElement.prototype;
    const setter = Object.getOwnPropertyDescriptor(proto, "value").set;
    setter.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
  }

  window.__miraRun = function (reqId, op) {
    let result;
    try {
      if (op.op === "snapshot") {
        result = {
          url: location.href,
          title: document.title,
          nodes: collect(),
        };
      } else if (op.op === "click") {
        const el = locate(op.target);
        if (!el) result = { error: "element not found: " + op.target };
        else {
          el.scrollIntoView({ block: "center", inline: "center" });
          if (el.focus) el.focus();
          el.click();
          result = { ok: true };
        }
      } else if (op.op === "fill") {
        const el = locate(op.target);
        if (!el) result = { error: "element not found: " + op.target };
        else {
          el.focus && el.focus();
          setNativeValue(el, op.value);
          result = { ok: true };
        }
      } else if (op.op === "eval") {
        result = { value: (0, eval)(op.js) };
      } else {
        result = { error: "unknown op " + op.op };
      }
    } catch (e) {
      result = { error: String(e) };
    }
    let json;
    try {
      json = JSON.stringify(result);
      if (json === undefined) json = "null";
    } catch {
      json = JSON.stringify({ error: "result not serializable" });
    }
    const b64 = btoa(unescape(encodeURIComponent(json)))
      .replace(/\+/g, "-")
      .replace(/\//g, "_")
      .replace(/=+$/, "");
    // Oversized results are truncated; agents get a marker instead of junk.
    const capped =
      b64.length > 400000
        ? btoa(unescape(encodeURIComponent(JSON.stringify({ error: "result too large" }))))
        : b64;
    location.href = "mira-result://r/" + reqId + "/" + capped;
  };
})();
