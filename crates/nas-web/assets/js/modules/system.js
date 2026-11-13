// 12. Módulo: Actualizaciones y Reinicio
// ==============================================================================
async function loadUpdates() {
  const container = document.getElementById('updates-container');
  if (!container) return;

  try {
    const res = await apiFetch('/api/system/updates');
    const u = res.data || res;
    if (!u) return;

    window.LAST_UPDATE_INFO = u;

    // Versión SemVer destacada
    const verTag = u.version ? (u.version.startsWith('v') ? u.version : `v${u.version}`) : 'v0.1.3';
    const latestVer = u.latest_version ? (u.latest_version.startsWith('v') ? u.latest_version : `v${u.latest_version}`) : verTag;

    // Badge verde de verificación criptográfica Ed25519
    const ed25519Badge = u.ed25519_verified
      ? `<span class="badge badge-ok" title="Manifiesto verificado criptográficamente con firma Ed25519" style="display:inline-flex; align-items:center; gap:5px; font-weight:600;">
           <svg class="icon" style="width:13px;height:13px;"><use href="#icon-check"></use></svg> Verificado con Ed25519
         </span>`
      : `<span class="badge badge-gray" title="Sin verificación Ed25519 registrada en el manifiesto local">Sin firma Ed25519</span>`;

    // Estado dinámico (Al día o actualización disponible)
    const statusBanner = u.has_updates
      ? `<div style="display:inline-flex; align-items:center; gap:8px; background:rgba(245,158,11,0.15); border:1px solid rgba(245,158,11,0.4); color:#fbbf24; padding:6px 14px; border-radius:6px; font-weight:600; font-size:13px;">
           <svg class="icon" style="width:15px;height:15px;"><use href="#icon-alert"></use></svg>
           <span>Nueva versión <strong>${escapeHtml(latestVer)}</strong> disponible</span>
           <button class="btn btn-primary btn-sm" onclick="openSystemUpdateModal()" style="margin-left:8px;">
             <svg class="icon" style="width:13px;height:13px;"><use href="#icon-download"></use></svg> Actualizar ahora
           </button>
         </div>`
      : `<div style="display:inline-flex; align-items:center; gap:6px; background:rgba(34,197,94,0.12); border:1px solid rgba(34,197,94,0.3); color:#22c55e; padding:6px 14px; border-radius:6px; font-weight:600; font-size:13px;">
           <svg class="icon" style="width:15px;height:15px;"><use href="#icon-check"></use></svg>
           <span>Sistema al día</span>
         </div>`;

    // Actualizar badge en la navegación lateral
    const navBadge = document.getElementById('badge-updates');
    if (navBadge) {
      if (u.has_updates) {
        navBadge.textContent = '¡Actualizar!';
        navBadge.className = 'nav-badge badge-warn';
        navBadge.style.color = '#fbbf24';
      } else {
        navBadge.textContent = 'Al día';
        navBadge.className = 'nav-badge';
        navBadge.style.color = '';
      }
    }

    // Cuadro colapsable con las notas de versión (Release Notes / Changelog)
    const notesContent = u.release_notes || 'Versión estable con optimizaciones para Debian 13, BTRFS Zstandard, Samba 4 y panel web Slate UI.';
    const changelogBlock = `
      <details style="margin-top:14px; background:var(--bg-surface); border:1px solid var(--border-color); border-radius:8px; padding:10px 14px;">
        <summary style="cursor:pointer; font-weight:600; color:var(--text-secondary); display:flex; align-items:center; justify-content:space-between; user-select:none;">
          <span style="display:inline-flex; align-items:center; gap:6px;">
            <svg class="icon" style="width:14px;height:14px;"><use href="#icon-logs"></use></svg>
            Notas de la versión (Changelog)
          </span>
          <span style="font-size:12px; color:var(--text-muted);">(clic para ver detalles)</span>
        </summary>
        <div style="margin-top:10px; padding-top:10px; border-top:1px solid var(--border-color); font-size:13px; line-height:1.6; color:var(--text-secondary); white-space:pre-wrap;">${escapeHtml(notesContent)}</div>
      </details>
    `;

    container.innerHTML = `
      <div style="display:flex; flex-direction:column; gap:16px;">
        <div style="display:flex; align-items:center; justify-content:space-between; flex-wrap:wrap; gap:12px;">
          <div style="display:flex; align-items:center; gap:10px; flex-wrap:wrap;">
            <span style="font-size:26px; font-weight:700; color:var(--text-primary); letter-spacing:-0.5px;">${escapeHtml(verTag)}</span>
            ${ed25519Badge}
            <span class="badge badge-blue">${escapeHtml(u.channel || 'stable')}</span>
          </div>
          <div>
            ${statusBanner}
          </div>
        </div>

        <div class="card-grid-3" style="gap:10px;">
          <div class="info-row" style="padding:8px 12px; background:var(--bg-surface); border-radius:6px; border:1px solid var(--border-color);">
            <span class="info-label">Versión instalada:</span>
            <strong class="info-val"><code>${escapeHtml(u.version || '0.1.3')}</code> (${escapeHtml(u.installed_commit || '')})</strong>
          </div>
          <div class="info-row" style="padding:8px 12px; background:var(--bg-surface); border-radius:6px; border:1px solid var(--border-color);">
            <span class="info-label">Versión más reciente:</span>
            <strong class="info-val"><code>${escapeHtml(latestVer)}</code></strong>
          </div>
          <div class="info-row" style="padding:8px 12px; background:var(--bg-surface); border-radius:6px; border:1px solid var(--border-color);">
            <span class="info-label">Fecha de publicación:</span>
            <strong class="info-val">${escapeHtml(u.commit_date || 'N/A')}</strong>
          </div>
        </div>

        ${changelogBlock}
      </div>
    `;
  } catch (e) {
    container.innerHTML = '<p style="color:var(--accent-danger);">Error al consultar actualizaciones: ' + escapeHtml(e.message) + '</p>';
  }
}

function openSystemUpdateModal() {
  const uptodateNotice = document.getElementById('system-update-uptodate-notice');
  if (uptodateNotice) {
    uptodateNotice.style.display = (window.LAST_UPDATE_INFO && !window.LAST_UPDATE_INFO.has_updates) ? 'flex' : 'none';
  }

  const progressDiv = document.getElementById('system-update-progress');
  const bar = document.getElementById('system-update-progress-bar');
  const log = document.getElementById('system-update-log');
  const stepLabel = document.getElementById('system-update-step-label');
  const spinner = document.getElementById('system-update-spinner');
  const footer = document.getElementById('system-update-footer');

  if (progressDiv) progressDiv.style.display = 'none';
  if (bar) {
    bar.style.width = '0%';
    bar.style.background = 'var(--accent-primary)';
  }
  if (log) {
    log.style.display = 'none';
    log.textContent = '';
  }
  if (stepLabel) {
    stepLabel.textContent = 'Iniciando actualización...';
    stepLabel.style.color = '';
  }
  if (spinner) spinner.style.display = '';

  if (footer) {
    footer.innerHTML = `
      <button type="button" class="btn btn-secondary" id="btn-cancel-system-update" onclick="closeModal('modal-system-update')">Cancelar</button>
      <button type="button" class="btn btn-primary" id="btn-start-system-update" onclick="startSystemUpdate()">
        <svg class="icon"><use href="#icon-download"></use></svg> Iniciar actualización ahora
      </button>
    `;
  }

  openModal('modal-system-update');
}

function applySystemUpdate() {
  openSystemUpdateModal();
}

async function startSystemUpdate() {
  const startBtn = document.getElementById('btn-start-system-update');
  const cancelBtn = document.getElementById('btn-cancel-system-update');
  const progressDiv = document.getElementById('system-update-progress');
  const bar = document.getElementById('system-update-progress-bar');
  const log = document.getElementById('system-update-log');
  const stepLabel = document.getElementById('system-update-step-label');
  const spinner = document.getElementById('system-update-spinner');
  const footer = document.getElementById('system-update-footer');

  if (startBtn) {
    startBtn.disabled = true;
    startBtn.classList.add('is-loading');
  }
  if (cancelBtn) {
    cancelBtn.disabled = true;
  }
  if (progressDiv) {
    progressDiv.style.display = 'block';
  }

  // Fases visuales informativas
  const phases = [
    { pct: '15%', text: 'Fase 1/5: Consultando última release y descargando paquete en caché' },
    { pct: '35%', text: 'Fase 2/5: Verificando suma SHA-256 y firma criptográfica Ed25519' },
    { pct: '55%', text: 'Fase 3/5: Extrayendo release y promoviendo enlace atómico A/B' },
    { pct: '75%', text: 'Fase 4/5: Instalando binarios, helpers y version.json activo' },
    { pct: '90%', text: 'Fase 5/5: Validando Nginx, recargando nas-web y comprobando arranque' },
  ];

  let phaseIndex = 0;
  if (bar) bar.style.width = phases[0].pct;
  if (stepLabel) stepLabel.textContent = phases[0].text;

  const phaseInterval = setInterval(() => {
    phaseIndex++;
    if (phaseIndex < phases.length) {
      if (bar) bar.style.width = phases[phaseIndex].pct;
      if (stepLabel) stepLabel.textContent = phases[phaseIndex].text;
    }
  }, 1200);

  try {
    const res = await apiFetch('/api/system/update/run', { method: 'POST' });

    const rawOut = res.data?.output;
    const outputText = typeof rawOut === 'string'
      ? rawOut
      : (rawOut ? JSON.stringify(rawOut, null, 2) : (res.message || 'Sincronización finalizada correctamente.'));
    if (log) {
      log.style.display = 'block';
      log.textContent = outputText;
    }

    let finalState = null;
    if (res.data?.queued) {
      const unitName = res.data?.unit || 'nas-system-update';
      let sawActive = false;
      let transientErrors = 0;

      for (let attempt = 0; attempt < 72; attempt++) {
        await new Promise((resolve) => setTimeout(resolve, 2500));
        try {
          const poll = await apiFetch('/api/system/updates', { silentToast: true });
          const d = poll.data || {};
          transientErrors = 0;

          if (d.update_in_progress) {
            sawActive = true;
            if (stepLabel) {
              stepLabel.textContent = `Actualizando en segundo plano (${unitName})...`;
            }
            continue;
          }

          if (sawActive || attempt >= 1) {
            finalState = d;
            break;
          }
        } catch (pollErr) {
          if (pollErr && pollErr.message === 'Sesión expirada.') {
            throw pollErr;
          }
          sawActive = true;
          transientErrors++;
          if (stepLabel) {
            stepLabel.textContent = 'Fase 5/5: Reiniciando servicio nas-web y comprobando disponibilidad...';
          }
          if (transientErrors >= 24) {
            throw new Error(`El panel no volvió a responder tras reiniciar ${unitName}.`);
          }
        }
      }

      if (!finalState) {
        throw new Error(`La unidad ${unitName} sigue ejecutándose. Revise journalctl -u ${unitName}.`);
      }
    }

    clearInterval(phaseInterval);

    const stillPending = Boolean(finalState && finalState.has_updates);
    if (bar) {
      bar.style.width = '100%';
      bar.style.background = stillPending ? '#f59e0b' : '#22c55e';
    }
    if (spinner) {
      spinner.style.display = 'none';
    }
    if (stepLabel) {
      stepLabel.innerHTML = stillPending
        ? '<span style="color:#f59e0b;">⚠ La unidad finalizó, pero siguen figurando actualizaciones pendientes</span>'
        : '<span style="color:#22c55e;">✔ Actualización completada con éxito</span>';
    }

    if (finalState && log) {
      log.textContent = `${outputText}\nVersión activa reportada: v${finalState.version || res.data?.version || 'desconocida'}`;
    }

    if (footer) {
      footer.innerHTML = `
        <button type="button" class="btn btn-secondary" onclick="closeModal('modal-system-update')">Cerrar</button>
        <button type="button" class="btn btn-primary" onclick="window.location.reload()">
          <svg class="icon"><use href="#icon-refresh"></use></svg> Recargar panel (Ctrl+F5)
        </button>
      `;
    }

    showToast(
      stillPending
        ? 'La actualización finalizó; verifique el estado de la versión o journalctl -u nas-system-update.'
        : 'Actualización del sistema completada con éxito.',
      stillPending ? 'warning' : 'success',
    );
    loadUpdates();
  } catch (e) {
    clearInterval(phaseInterval);
    if (bar) {
      bar.style.width = '100%';
      bar.style.background = '#ef4444';
    }
    if (spinner) {
      spinner.style.display = 'none';
    }
    if (stepLabel) {
      stepLabel.innerHTML = `<span style="color:#ef4444;">✘ Error: ${escapeHtml(e.message)}</span>`;
    }
    if (log) {
      log.style.display = 'block';
      log.textContent = e.message;
    }
    if (footer) {
      footer.innerHTML = `
        <button type="button" class="btn btn-secondary" onclick="closeModal('modal-system-update')">Cerrar</button>
        <button type="button" class="btn btn-primary" onclick="startSystemUpdate()">Reintentar</button>
      `;
    }
    showToast('Error al actualizar el sistema: ' + e.message, 'error');
  }
}
