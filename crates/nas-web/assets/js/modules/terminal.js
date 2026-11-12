// 11. Módulo: Consola Terminal Web Real (Bash Interactivo)
// ==============================================================================
function ansiToHtml(text) {
  if (!text) return '';
  let str = text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');

  const colorMap = {
    '30': 'color:#1e293b',
    '31': 'color:#ef4444',
    '32': 'color:#10b981',
    '33': 'color:#f59e0b',
    '34': 'color:#3b82f6',
    '35': 'color:#d946ef',
    '36': 'color:#06b6d4',
    '37': 'color:#e2e8f0',
    '90': 'color:#64748b',
    '91': 'color:#f87171',
    '92': 'color:#34d399',
    '93': 'color:#fbbf24',
    '94': 'color:#60a5fa',
    '95': 'color:#e879f9',
    '96': 'color:#22d3ee',
    '97': 'color:#ffffff',
    '1': 'font-weight:700',
    '4': 'text-decoration:underline',
  };

  let openSpans = 0;
  str = str.replace(/\x1b\[([0-9;]*)m/g, (match, codes) => {
    if (!codes || codes === '0') {
      const res = '</span>'.repeat(openSpans);
      openSpans = 0;
      return res;
    }
    const parts = codes.split(';');
    const styles = [];
    parts.forEach(p => {
      if (colorMap[p]) styles.push(colorMap[p]);
    });
    if (styles.length > 0) {
      openSpans++;
      return `<span style="${styles.join(';')}">`;
    }
    return '';
  });
  if (openSpans > 0) {
    str += '</span>'.repeat(openSpans);
  }
  return str;
}

function initTerminal() {
  const input = document.getElementById('terminal-input');
  const output = document.getElementById('terminal-output');
  if (!input || !output) return;

  input.addEventListener('keydown', async (e) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      const cmd = input.value;
      input.value = '';
      if (cmd.trim() === '') {
        await sendTerminalKey('ENTER');
        return;
      }
      AppState.terminal.history.push(cmd);
      AppState.terminal.historyIndex = AppState.terminal.history.length;
      await sendTerminalLine(cmd);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (AppState.terminal.history.length > 0 && AppState.terminal.historyIndex > 0) {
        AppState.terminal.historyIndex--;
        input.value = AppState.terminal.history[AppState.terminal.historyIndex];
      }
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (AppState.terminal.historyIndex < AppState.terminal.history.length - 1) {
        AppState.terminal.historyIndex++;
        input.value = AppState.terminal.history[AppState.terminal.historyIndex];
      } else {
        AppState.terminal.historyIndex = AppState.terminal.history.length;
        input.value = '';
      }
    } else if (e.key === 'Tab') {
      e.preventDefault();
      const currentVal = input.value;
      if (currentVal) {
        await apiFetch('/api/terminal/send', {
          method: 'POST',
          body: JSON.stringify({ data: currentVal, raw: true }),
          silentToast: true,
        });
        input.value = '';
      }
      await sendTerminalKey('TAB');
    } else if (e.ctrlKey && (e.key === 'c' || e.key === 'C')) {
      const selection = window.getSelection().toString();
      if (selection && selection.length > 0) {
        return;
      }
      e.preventDefault();
      await sendTerminalKey('CTRL_C');
    }
  });

  output.addEventListener('click', () => input.focus());

  let resizeTimer = null;
  window.addEventListener('resize', () => {
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(() => {
      const view = document.getElementById('view-terminal');
      if (view && view.classList.contains('active')) {
        resizeTerminal();
      }
    }, 400);
  });
}

async function startTerminalSession() {
  if (AppState.terminal.ptyStarted) {
    startTerminalPolling();
    return;
  }
  AppState.terminal.ptyStarted = true;
  try {
    await apiFetch('/api/terminal/session', {
      method: 'POST',
      body: JSON.stringify({ cols: 200, rows: 50 }),
      silentToast: true,
    });
    startTerminalPolling();
  } catch (e) {
    AppState.terminal.ptyStarted = false;
  }
}

function startTerminalPolling() {
  if (AppState.terminal.ptyTimer) return;
  AppState.terminal.ptyTimer = setInterval(refreshTerminalPane, 900);
  refreshTerminalPane();
}

function stopTerminalPolling() {
  if (AppState.terminal.ptyTimer) {
    clearInterval(AppState.terminal.ptyTimer);
    AppState.terminal.ptyTimer = null;
  }
}

async function refreshTerminalPane() {
  const output = document.getElementById('terminal-output');
  if (!output) return;
  try {
    const res = await apiFetch('/api/terminal/capture', { silentToast: true });
    const payload = res.data ?? res;
    const text = payload.output ?? '';
    const cwd = payload.cwd ?? '';
    const user = payload.user ?? '';

    if (cwd) {
      const barCwd = document.getElementById('term-bar-cwd');
      const promptCwd = document.getElementById('term-prompt-cwd');
      if (barCwd && barCwd.textContent !== cwd) barCwd.textContent = cwd;
      if (promptCwd && promptCwd.textContent !== cwd) promptCwd.textContent = cwd;
    }
    if (user) {
      const promptUser = document.getElementById('term-prompt-user');
      if (promptUser && promptUser.textContent !== user) promptUser.textContent = user;
    }

    const isNearBottom = (output.scrollHeight - output.scrollTop - output.clientHeight) < 60;
    output.innerHTML = ansiToHtml(text.replace(/\s+$/, '\n'));

    if (isNearBottom) {
      output.scrollTop = output.scrollHeight;
    }
  } catch (e) {
    // Silencioso: la sesión puede estar reiniciándose
  }
}

async function sendTerminalLine(cmd) {
  try {
    await apiFetch('/api/terminal/send', {
      method: 'POST',
      body: JSON.stringify({ data: cmd }),
      silentToast: true,
    });
    setTimeout(refreshTerminalPane, 150);
  } catch (e) {
    // Ya mostrado
  }
}

async function sendTerminalKey(token) {
  try {
    await apiFetch('/api/terminal/send', {
      method: 'POST',
      body: JSON.stringify({ data: token }),
      silentToast: true,
    });
    setTimeout(refreshTerminalPane, 150);
  } catch (e) {
    // Ya mostrado
  }
}

function clearTerminal() {
  sendTerminalLine('clear');
}

function runQuickCommand(cmd) {
  const input = document.getElementById('terminal-input');
  if (input) input.focus();
  AppState.terminal.history.push(cmd);
  AppState.terminal.historyIndex = AppState.terminal.history.length;
  sendTerminalLine(cmd);
}

async function reconnectTerminal() {
  stopTerminalPolling();
  AppState.terminal.ptyStarted = false;
  const output = document.getElementById('terminal-output');
  if (output) {
    output.textContent += '\n[Reconectando sesión interactiva de terminal PTY tmux...]\n';
  }
  try {
    await apiFetch('/api/terminal/kill', { method: 'POST', silentToast: true });
  } catch (e) { }
  await startTerminalSession();
  showToast('Sesión de terminal reconectada.', 'info');
}

async function resizeTerminal(cols, rows) {
  const container = document.getElementById('terminal-output');
  let c = cols;
  let r = rows;
  if (!c || !r) {
    const width = container ? container.clientWidth : 800;
    const height = container ? container.clientHeight : 400;
    c = Math.max(80, Math.floor(width / 8.5));
    r = Math.max(24, Math.floor(height / 18));
  }
  try {
    await apiFetch('/api/terminal/resize', {
      method: 'POST',
      body: JSON.stringify({ cols: c, rows: r }),
      silentToast: true,
    });
    showToast(`Dimensiones de terminal ajustadas (${c}x${r}).`, 'info');
  } catch (e) {
    // Silencioso
  }
}

// ==============================================================================
