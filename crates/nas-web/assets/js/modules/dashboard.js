// 4. Módulo: Dashboard & Métricas
// ==============================================================================
async function refreshDashboardMetrics() {
  const icon = document.getElementById('icon-refresh-metrics');
  if (icon) icon.classList.add('spin');

  try {
    const res = await apiFetch('/api/metrics');
    const d = res.data || res;

    if (d.system) {
      if (d.system.server_timestamp) {
        _serverClock.timeDiffMs = (d.system.server_timestamp * 1000) - Date.now();
        if (d.system.timezone && d.system.timezone !== _serverClock.tzIana) {
          _serverClock.tzIana = d.system.timezone;
          try {
            _serverClock.formatter = new Intl.DateTimeFormat('en-GB', {
              timeZone: _serverClock.tzIana,
              hour: '2-digit',
              minute: '2-digit',
              second: '2-digit',
              hour12: false
            });
          } catch (e) {
            _serverClock.formatter = null;
          }
        }
        if (typeof d.system.timezone_offset === 'number') {
          _serverClock.tzOffsetSec = d.system.timezone_offset;
        }
        updateServerClockDisplay(d.system.timezone_abbr);
      }
      const ramVal = document.getElementById('kpi-ram-val');
      const ramBar = document.getElementById('kpi-ram-bar');
      if (ramVal) ramVal.innerHTML = `${d.system.ram_used_gb} GB <small style="font-size:12px; color:var(--text-secondary);">/ ${d.system.ram_total_gb} GB</small>`;
      if (ramBar) ramBar.style.width = `${d.system.ram_usage_pct}%`;
      const cpuText = document.getElementById('kpi-cpu-text');
      if (cpuText) {
        const cpuPct = (d.system.cpu_usage_pct !== undefined && d.system.cpu_usage_pct !== null) ? d.system.cpu_usage_pct : 0;
        const cpuModel = d.system.cpu_model || 'x86_64';
        cpuText.textContent = `CPU: ${cpuPct}% • ${cpuModel}`;
      }
    }

    if (d.storage) {
      const sVal = document.getElementById('kpi-storage-val');
      const sBar = document.getElementById('kpi-storage-bar');
      const sBadge = document.getElementById('kpi-storage-badge');
      const sAlert = document.getElementById('kpi-storage-alert');
      const sAlertText = document.getElementById('kpi-storage-alert-text');
      const sCard = document.getElementById('kpi-card-storage');

      if (sVal) sVal.innerHTML = `${d.storage.used_gb} GB <small style="font-size:12px; color:var(--text-secondary);">/ ${d.storage.total_gb} GB</small>`;
      const pct = Number(d.storage.usage_percent || 0);
      if (sBar) {
        sBar.style.width = `${Math.min(100, Math.max(0, pct))}%`;
      }

      if (pct >= 90) {
        if (sBar) sBar.style.background = 'var(--accent-danger)';
        if (sBadge) {
          sBadge.className = 'badge badge-err';
          sBadge.style.display = 'inline-block';
          sBadge.textContent = `Crítico (${pct}%)`;
        }
        if (sAlert && sAlertText) {
          sAlert.className = 'alert-box alert-box-danger';
          sAlert.style.display = 'flex';
          sAlertText.textContent = `Capacidad crítica superada (${pct}% en uso). A partir del 95% de uso las tareas de respaldo se cancelarán.`;
        }
        if (sCard) sCard.style.borderColor = 'var(--accent-danger)';
      } else if (pct >= 80) {
        if (sBar) sBar.style.background = 'var(--accent-warning)';
        if (sBadge) {
          sBadge.className = 'badge badge-warn';
          sBadge.style.display = 'inline-block';
          sBadge.textContent = `Alerta preventiva (${pct}%)`;
        }
        if (sAlert && sAlertText) {
          sAlert.className = 'alert-box alert-box-warning';
          sAlert.style.display = 'flex';
          sAlertText.textContent = `Umbral preventivo superado (${pct}% en /srv/nas). Al superar el 85%, el sistema generará alertas operativas.`;
        }
        if (sCard) sCard.style.borderColor = 'var(--accent-warning)';
      } else {
        if (sBar) sBar.style.background = 'var(--accent-primary)';
        if (sBadge) {
          sBadge.style.display = 'none';
        }
        if (sAlert) {
          sAlert.style.display = 'none';
        }
        if (sCard) sCard.style.borderColor = '';
      }
    }

    if (d.counts) {
      const bShares = document.getElementById('badge-shares');
      const bBackups = document.getElementById('badge-backups');
      const bUsers = document.getElementById('badge-users');
      if (bShares) bShares.textContent = d.counts.shares;
      if (bBackups) bBackups.textContent = d.counts.backups;
      if (bUsers) bUsers.textContent = d.counts.users;
    }

    if (d.services && Array.isArray(d.services)) {
      const netBadges = document.getElementById('kpi-network-badges');
      if (netBadges) {
        netBadges.innerHTML = d.services
          .filter(s => ['smbd', 'wsdd2', 'nginx', 'nas-web'].includes(s.service) || (s.service && s.service.includes('fpm')))
          .map(s => {
            const label = s.service === 'nas-web' ? 'nas-web (Rust)' : ((s.service && s.service.includes('fpm')) ? 'php-fpm' : s.service);
            const cls = s.active ? 'badge-ok' : 'badge-danger';
            const statusTxt = s.active ? ((s.service && s.service.includes('fpm')) ? 'ondemand' : 'OK') : 'OFF';
            return `<span class="badge ${cls}">${escapeHtml(label)} ${statusTxt}</span>`;
          })
          .join(' ');
      }
    }

    loadDashboardActivity();
  } catch (e) {
    console.error('Error actualizando métricas:', e);
  } finally {
    if (icon) setTimeout(() => icon.classList.remove('spin'), 500);
  }
}

async function loadDashboardActivity() {
  const feed = document.getElementById('dashboard-activity-feed');
  if (!feed) return;

  try {
    const res = await apiFetch('/api/logs?limit=5');
    const logs = res.data || [];
    if (logs.length === 0) {
      feed.innerHTML = '<p style="color:var(--text-secondary);">Sin actividad reciente registrada.</p>';
      return;
    }

    feed.innerHTML = logs.map(l => {
      const label = l.action_label || l.event_label || l.action || l.event || l.unit || 'Evento';
      const detail = l.message || (l.target ? `${l.target}` : '');
      const badge = l.badge || 'blue';
      const tsShort = (l.timestamp || '').substring(11, 19) || l.timestamp || '';
      return `
        <div style="font-size:12px; border-bottom:1px solid var(--border-color); padding:4px 0; display:flex; justify-content:space-between; align-items:center; gap:8px;">
          <div style="display:flex; align-items:center; gap:6px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;">
            <span class="badge badge-${badge}">${escapeHtml(label)}</span>
            <span style="color:var(--text-secondary);">${escapeHtml(detail)}</span>
          </div>
          <span style="font-size:10.5px; color:var(--text-muted); font-family:monospace; white-space:nowrap;">${escapeHtml(tsShort)}</span>
        </div>
      `;
    }).join('');
  } catch (e) {
    feed.innerHTML = '<p style="color:var(--accent-danger);">No se pudo cargar la actividad reciente.</p>';
  }
}

// ==============================================================================
