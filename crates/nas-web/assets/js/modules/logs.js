// 10. Módulo: Registros del Sistema (Logs & Auditoría)
// ==============================================================================
function setLogSource(source) {
  AppState.logSource = source;
  document.querySelectorAll('#logs-filter-chips .chip-btn').forEach(btn => {
    btn.classList.toggle('active', btn.getAttribute('data-source') === source);
  });
  loadLogs();
}

function debounceLogSearch() {
  if (AppState.logSearchTimer) {
    clearTimeout(AppState.logSearchTimer);
  }
  AppState.logSearchTimer = setTimeout(() => {
    loadLogs();
  }, 300);
}

async function loadLogs() {
  const tbody = document.getElementById('logs-table-body');
  if (!tbody) return;

  const source = AppState.logSource || 'all';
  const limitSelect = document.getElementById('logs-limit-select');
  const limit = limitSelect ? parseInt(limitSelect.value, 10) || 100 : 100;
  const searchInput = document.getElementById('logs-search-input');
  const query = searchInput ? searchInput.value.trim() : '';
  const groupChk = document.getElementById('chk-group-logs');
  const grouped = groupChk ? (groupChk.checked ? 1 : 0) : 1;

  try {
    let url = `/api/logs?source=${encodeURIComponent(source)}&limit=${limit}&grouped=${grouped}`;
    if (query) {
      url += `&q=${encodeURIComponent(query)}`;
    }

    const res = await apiFetch(url);
    const logs = res.data || [];
    AppState.logsCache = logs;

    if (logs.length === 0) {
      tbody.innerHTML = '<tr><td colspan="7" style="text-align:center; padding:24px; color:var(--text-muted);">No se encontraron registros de auditoría para los criterios seleccionados.</td></tr>';
      return;
    }

    tbody.innerHTML = logs.map((l, index) => {
      const srcBadge = logOriginBadge(l);
      const actionLabel = l.action_label || l.event_label || l.action || l.event || l.unit || 'registro';
      const badgeType = l.badge || 'blue';
      const targetText = l.target || l.task || l.unit || '-';
      const status = (l.status || 'OK').toUpperCase();
      const statusBadge = (status === 'SUCCESS' || status === 'OK' || status === 'ALLOW') ? 'badge-ok' : ((status === 'FAILED' || status === 'ERR' || status === 'BLOCK') ? 'badge-err' : 'badge-warn');
      const countBadge = (l.count && l.count > 1)
        ? `<span class="badge badge-antispam" title="Evento idéntico repetido ${l.count} veces">x${l.count}</span>`
        : '';
      const humanSummary = l.human || l.message || l.raw || '-';

      return `
        <tr>
          <td class="table-logs-timestamp" title="${escapeHtml(l.timestamp || '')}"><code>${escapeHtml(formatLogTimestamp(l.timestamp))}</code></td>
          <td>${srcBadge}</td>
          <td>${logActorCell(l)}</td>
          <td><span class="badge badge-${badgeType}">${escapeHtml(actionLabel)}</span>${countBadge}</td>
          <td class="table-logs-target"><code>${escapeHtml(targetText)}</code></td>
          <td><span class="badge ${statusBadge}">${escapeHtml(status)}</span></td>
          <td class="table-logs-details">
            <button type="button" class="btn btn-secondary btn-sm" style="padding:2px 6px;" title="${escapeHtml(humanSummary)}" aria-label="Ver detalle del registro" onclick="openLogDetail(${index})">
              <svg class="icon icon-sm"><use href="#icon-eye"></use></svg>
            </button>
          </td>
        </tr>
      `;
    }).join('');
  } catch (e) {
    tbody.innerHTML = `<tr><td colspan="7" style="text-align:center; color:var(--accent-danger); padding:16px;">Error al consultar registros de auditoría: ${escapeHtml(e.message)}</td></tr>`;
  }
}

function formatLogTimestamp(value) {
  if (!value) return '-';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  try {
    return new Intl.DateTimeFormat('es-MX', {
      day: '2-digit',
      month: 'short',
      year: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
      hour12: false,
    }).format(date);
  } catch (e) {
    return value;
  }
}

function serverHostname() {
  const node = document.getElementById('masthead-hostname');
  const name = node ? node.textContent.trim() : '';
  return name || window.location.hostname || 'Servidor';
}

function logOriginBadge(entry) {
  const label = entry.origin || 'Sistema';
  let badge = 'gray';
  switch (entry.source) {
    case 'samba_audit':
      badge = 'purple';
      break;
    case 'admin':
      badge = (entry.action || '').startsWith('terminal') ? 'warn' : 'blue';
      break;
    case 'backup':
      badge = 'ok';
      break;
    default:
      badge = 'gray';
  }
  return `<span class="badge badge-${badge}">${escapeHtml(label)}</span>`;
}

function logActorCell(entry) {
  const user = entry.user || entry.task || 'sistema';
  const place = entry.ip || entry.origin_detail || serverHostname();
  return `<span class="table-logs-user">${escapeHtml(user)}</span><span class="table-logs-ip"><code>${escapeHtml(place)}</code></span>`;
}

function openLogDetail(index) {
  const entry = (AppState.logsCache || [])[index];
  if (!entry) return;
  const summary = entry.human || entry.message || entry.raw || '-';
  const summaryEl = document.getElementById('log-detail-summary');
  if (summaryEl) summaryEl.textContent = summary;
  const codeEl = document.getElementById('log-detail-code');
  if (codeEl) {
    if (entry.code) {
      codeEl.textContent = entry.code;
      codeEl.hidden = false;
    } else {
      codeEl.textContent = '';
      codeEl.hidden = true;
    }
  }
  const rawEl = document.getElementById('log-detail-raw');
  if (rawEl) rawEl.textContent = entry.raw || summary;
  const copyEl = document.getElementById('log-detail-copy');
  if (copyEl) copyEl.dataset.raw = entry.raw || '';
  openModal('modal-log-detail');
}

function copyLogDetail() {
  const button = document.getElementById('log-detail-copy');
  const text = button ? (button.dataset.raw || '') : '';
  if (!text) {
    showToast('No hay texto técnico para copiar.', 'warning');
    return;
  }
  if (!navigator.clipboard || !navigator.clipboard.writeText) {
    showToast('El navegador no permite copiar automáticamente.', 'warning');
    return;
  }
  navigator.clipboard.writeText(text)
    .then(() => showToast('Texto técnico copiado.', 'success'))
    .catch(() => showToast('No se pudo copiar el texto.', 'error'));
}

function exportOrCopyLogs() {
  const logs = AppState.logsCache || [];
  if (logs.length === 0) {
    showToast('No hay registros cargados para copiar.', 'warning');
    return;
  }

  const lines = logs.map(l => {
    const ts = l.timestamp || '';
    const src = (l.source || 'system').toUpperCase();
    const user = l.user || l.task || 'sistema';
    const ip = l.ip || '-';
    const countStr = (l.count && l.count > 1) ? ` [x${l.count}]` : '';
    const action = (l.action_label || l.event_label || l.action || '-') + countStr;
    const target = l.target || '-';
    const status = l.status || 'OK';
    const msg = l.human || l.message || (l.details ? (typeof l.details === 'object' ? JSON.stringify(l.details) : l.details) : '-');
    return `[${ts}] [${src}] [${user}@${ip}] [${action}] [${target}] [${status}] ${msg}`;
  });

  const text = lines.join('\n');

  if (navigator.clipboard && navigator.clipboard.writeText) {
    navigator.clipboard.writeText(text).then(() => {
      showToast(`${logs.length} registros copiados al portapapeles.`, 'success');
    }).catch(() => {
      fallbackCopyText(text, logs.length);
    });
  } else {
    fallbackCopyText(text, logs.length);
  }
}

function fallbackCopyText(text, count) {
  const ta = document.createElement('textarea');
  ta.value = text;
  ta.style.position = 'fixed';
  ta.style.left = '-9999px';
  document.body.appendChild(ta);
  ta.select();
  try {
    document.execCommand('copy');
    showToast(`${count} registros copiados al portapapeles.`, 'success');
  } catch (err) {
    showToast('No se pudo copiar al portapapeles.', 'error');
  } finally {
    document.body.removeChild(ta);
  }
}

async function verifyAuditIntegrity() {
  try {
    showToast('Verificando cadena criptográfica SHA-256 de auditoría...', 'info');
    const res = await apiFetch('/api/system/audit/verify');
    const data = res.data || res;
    if (data.valid) {
      const hashShort = data.last_hash && data.last_hash !== '0' ? data.last_hash.substring(0, 16) + '...' : 'vacía';
      showToast(`Auditoría íntegra: ${data.count} registros validados (Hash: ${hashShort})`, 'success', 6000);
    } else {
      showToast(`¡Alerta! Integridad de auditoría comprometida: ${data.error || 'Corrupción detectada'}`, 'error', 10000);
    }
  } catch (e) {
    showToast(`Error al verificar auditoría: ${e.message}`, 'error');
  }
}

// ==============================================================================
