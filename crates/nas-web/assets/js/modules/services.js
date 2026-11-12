// 9. Módulo: Servicios y Demonios
// ==============================================================================
async function loadServices() {
  const tbody = document.getElementById('services-table-body');
  if (!tbody) return;

  try {
    const res = await apiFetch('/api/services');
    const services = res.data || [];

    tbody.innerHTML = services.map(s => `
      <tr>
        <td><strong><code>${escapeHtml(s.service)}</code></strong></td>
        <td>${escapeHtml(s.name)}</td>
        <td>${s.active ? '<span class="badge badge-ok">Activo</span>' : '<span class="badge badge-danger">Inactivo</span>'}</td>
        <td style="text-align:right; white-space:nowrap;">
          ${s.active ? `
            <button class="btn btn-secondary btn-sm" onclick="reloadService('${escapeHtml(s.service)}')" title="Recargar configuración">
              <svg class="icon icon-sm"><use href="#icon-refresh"></use></svg> Recargar
            </button>
            <button class="btn btn-secondary btn-sm" onclick="restartService('${escapeHtml(s.service)}')" title="Reiniciar demonio">
              <svg class="icon icon-sm"><use href="#icon-refresh"></use></svg> Reiniciar
            </button>
            <button class="btn btn-secondary btn-sm" style="color:var(--accent-danger);" onclick="stopService('${escapeHtml(s.service)}')" title="Detener demonio">
              <svg class="icon icon-sm"><use href="#icon-stop"></use></svg> Detener
            </button>
          ` : `
            <button class="btn btn-primary btn-sm" onclick="startService('${escapeHtml(s.service)}')" title="Iniciar demonio">
              <svg class="icon icon-sm"><use href="#icon-play"></use></svg> Iniciar
            </button>
          `}
        </td>
      </tr>
    `).join('');
  } catch (e) {
    tbody.innerHTML = `<tr><td colspan="4" style="text-align:center; color:var(--accent-danger);">Error al cargar servicios: ${escapeHtml(e.message)}</td></tr>`;
  }
}

async function manageServiceAction(service, action) {
  try {
    await apiFetch('/api/services/manage', {
      method: 'POST',
      body: JSON.stringify({ service, action }),
    });

    const labels = {
      start: 'iniciado',
      stop: 'detenido',
      restart: 'reiniciado',
      reload: 'recargado',
    };
    showToast(`Servicio [${service}] ${labels[action] || action} con éxito.`, 'success');
    loadServices();
  } catch (e) {
    // Ya mostrado por apiFetch
  }
}

async function startService(service) { return manageServiceAction(service, 'start'); }
async function stopService(service) { return manageServiceAction(service, 'stop'); }
async function reloadService(service) { return manageServiceAction(service, 'reload'); }
async function restartService(service) { return manageServiceAction(service, 'restart'); }

async function runDiagnosticTest(type) {
  const resultEl = document.getElementById('diag-test-result');
  if (resultEl) {
    resultEl.innerHTML = `<span style="color:var(--text-muted);"><svg class="icon icon-sm spin"><use href="#icon-refresh"></use></svg> Ejecutando prueba ${escapeHtml(type)}...</span>`;
  }
  showToast(`Ejecutando prueba ${type}...`, 'info');

  try {
    const res = await apiFetch('/api/diagnostics/run', {
      method: 'POST',
      body: JSON.stringify({ type }),
    });

    if (resultEl) {
      const isOk = res.status === 'success';
      const color = isOk ? 'var(--accent-success)' : 'var(--accent-danger)';
      resultEl.innerHTML = `<span style="color:${color}; font-weight:600;">[${isOk ? 'OK' : 'FALLO'}] ${escapeHtml(res.message || 'Prueba completada')}</span>`;
    }
    showToast(res.message || 'Prueba de diagnóstico completada.', 'success');
  } catch (e) {
    if (resultEl) {
      resultEl.innerHTML = `<span style="color:var(--accent-danger); font-weight:600;">[ERROR] ${escapeHtml(e.message)}</span>`;
    }
    showToast(`Error: ${e.message}`, 'error');
  }
}

// ==============================================================================
