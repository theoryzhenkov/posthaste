pub fn bridge_initialization_script() -> &'static str {
    r#"
(function () {
  if (window.__POSTHASTE_E2E_BRIDGE__) {
    return;
  }
  window.__POSTHASTE_E2E_BRIDGE__ = true;

  const commandEvent = 'posthaste://e2e-command';
  const resultCommand = 'posthaste_e2e_result';

  function sleep(ms) {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }

  function visible(selector) {
    const element = document.querySelector(selector);
    if (!element) {
      return false;
    }
    const rect = element.getBoundingClientRect();
    const style = getComputedStyle(element);
    return (
      rect.width > 0 &&
      rect.height > 0 &&
      style.visibility !== 'hidden' &&
      style.display !== 'none' &&
      Number.parseFloat(style.opacity || '1') > 0
    );
  }

  async function waitForElement(selector, timeoutMs, requireVisible) {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      const element = document.querySelector(selector);
      if (element && (!requireVisible || visible(selector))) {
        return element;
      }
      await sleep(50);
    }
    throw new Error(`timeout (${timeoutMs}ms) waiting for ${selector}`);
  }

  function canParseAsExpression(script) {
    try {
      new Function(`return (async () => (${script}))();`);
      return true;
    } catch (error) {
      if (error instanceof SyntaxError) {
        return false;
      }
      throw error;
    }
  }

  async function evaluateExpression(script) {
    if (canParseAsExpression(script)) {
      return await (0, eval)(`(async () => (${script}))()`);
    }
    return await (0, eval)(`(async () => { ${script} })()`);
  }

  async function waitForFunction(expression, timeoutMs) {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      if (await evaluateExpression(expression)) {
        return null;
      }
      await sleep(50);
    }
    throw new Error(`timeout (${timeoutMs}ms) waiting for function`);
  }

  async function execute(command) {
    const timeoutMs = command.timeout_ms ?? 5000;
    switch (command.type) {
      case 'eval':
        return await evaluateExpression(command.script);
      case 'url':
        return window.location.href;
      case 'title':
        return document.title;
      case 'content':
        return document.documentElement.outerHTML;
      case 'is_visible':
        return visible(command.selector);
      case 'is_disabled': {
        const element = document.querySelector(command.selector);
        return !element || element.disabled === true || element.hasAttribute('disabled');
      }
      case 'is_checked': {
        const element = document.querySelector(command.selector);
        return !!(element && element.checked);
      }
      case 'count':
        return document.querySelectorAll(command.selector).length;
      case 'wait_for_selector':
        await waitForElement(command.selector, timeoutMs, true);
        return null;
      case 'wait_for_function':
        await waitForFunction(command.expression, timeoutMs);
        return null;
      case 'text_content': {
        const element = await waitForElement(command.selector, timeoutMs, false);
        return element.textContent;
      }
      case 'inner_text': {
        const element = await waitForElement(command.selector, timeoutMs, false);
        return element.innerText;
      }
      case 'inner_html': {
        const element = await waitForElement(command.selector, timeoutMs, false);
        return element.innerHTML;
      }
      case 'get_attribute': {
        const element = await waitForElement(command.selector, timeoutMs, false);
        return element.getAttribute(command.name);
      }
      case 'input_value': {
        const element = await waitForElement(command.selector, timeoutMs, false);
        return element.value || '';
      }
      case 'bounding_box': {
        const element = await waitForElement(command.selector, timeoutMs, false);
        const rect = element.getBoundingClientRect();
        return { x: rect.left, y: rect.top, width: rect.width, height: rect.height };
      }
      case 'click': {
        const element = await waitForElement(command.selector, timeoutMs, true);
        element.scrollIntoView({ block: 'center' });
        element.click();
        return null;
      }
      case 'fill': {
        const element = await waitForElement(command.selector, timeoutMs, true);
        element.focus();
        element.value = command.text ?? '';
        element.dispatchEvent(new Event('input', { bubbles: true }));
        element.dispatchEvent(new Event('change', { bubbles: true }));
        return null;
      }
      default:
        throw new Error(`unsupported e2e command: ${command.type}`);
    }
  }

  async function sendResult(payload) {
    if (window.__TAURI__ && window.__TAURI__.core) {
      await window.__TAURI__.core.invoke(resultCommand, payload);
      return;
    }
    if (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke) {
      await window.__TAURI_INTERNALS__.invoke(resultCommand, payload);
      return;
    }
    throw new Error('Tauri invoke API is unavailable');
  }

  async function register() {
    if (!window.__TAURI__ || !window.__TAURI__.event || !window.__TAURI__.core) {
      window.setTimeout(register, 10);
      return;
    }

    await window.__TAURI__.event.listen(commandEvent, async (event) => {
      const payload = event.payload || {};
      try {
        const data = await execute(payload.command || {});
        await sendResult({ id: payload.id, ok: true, data: data === undefined ? null : data });
      } catch (error) {
        await sendResult({
          id: payload.id,
          ok: false,
          error: String((error && error.message) || error || 'unknown e2e error'),
        });
      }
    });
  }

  register().catch((error) => console.error('PostHaste e2e bridge failed to initialize', error));
})();
"#
}
