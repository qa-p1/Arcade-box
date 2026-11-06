// Opt-in native package check; injected only with an isolated ARCADE_HOME.
addEventListener('DOMContentLoaded', async () => {
  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const wait = async (probe, description) => {
    const end = Date.now() + 20000;
    while (Date.now() < end) {
      const result = probe();
      if (result) return result;
      await new Promise(resolve => setTimeout(resolve, 50));
    }
    throw new Error(`Timed out: ${description}; ${document.body.innerText.slice(-1500)}`);
  };
  const setInput = (input, value) => {
    input.value = value;
    input.dispatchEvent(new Event('input', { bubbles: true }));
  };
  const scrollContent = async (description) => {
    const pane = await wait(() => document.querySelector('.shell-main'), description);
    if (!['auto', 'scroll'].includes(getComputedStyle(pane).overflowY)) throw new Error(`${description}: content has no vertical scroller`);
    const rect = pane.getBoundingClientRect();
    if (rect.bottom > innerHeight + 1 || rect.top < -1) throw new Error(`${description}: content extends outside window`);
    // Catalog mounting and native window mapping are asynchronous. Wait for
    // the full layout rather than inspecting the first inserted card.
    await wait(() => pane.clientHeight > 0 && pane.scrollHeight > pane.clientHeight + 2, `${description} overflowing layout`);
    pane.scrollTop = pane.scrollHeight;
    await wait(() => pane.scrollTop > 0 && Math.abs(pane.scrollHeight - pane.clientHeight - pane.scrollTop) <= 2, `${description} bottom`);
    pane.scrollTop = 0;
  };
  try {
    if (document.documentElement.dataset.surface === 'dashboard') {
      await wait(() => document.querySelector('.tool-card'), 'native catalog');
      await scrollContent('native tool catalog scroll');
      // Test the settings-page event handshake too: without dashboard event
      // permissions dashboard_ready never runs and page selection is lost.
      await invoke('open_dashboard', { page: 'settings' });
      await wait(() => document.querySelector('#settings-general'), 'dashboard Settings handoff');
      await scrollContent('native Settings scroll');
      await invoke('set_preference', { key: 'onboarding_complete', value: 'true' });
      await invoke('open_dashboard', { page: 'all' });
      const card = await wait(() => [...document.querySelectorAll('.tool-card')]
        .find(node => node.querySelector('.tool-name')?.textContent === 'Calculator'), 'calculator card');
      card.click();
    } else {
      await wait(() => document.querySelector('#tool-input'), 'dashboard to Island tool handoff');
      if (!document.querySelector('h1')?.textContent.includes('Calculator')) throw new Error('Wrong tool');
      setInput(document.querySelector('#tool-input'), '6*7');
      const run = await wait(() => {
        const button = document.querySelector('.run-button');
        return button && !button.disabled ? button : null;
      }, 'enabled Run');
      run.click();
      await wait(() => document.querySelector('.result-panel:not(.result-failed)')?.textContent.includes('42'), 'calculation result 42');
      await invoke('ui_check_report', { report: 'passed' });
    }
  } catch (error) {
    await invoke('ui_check_report', { report: String(error) });
  }
});
