// 13. Utilidades Generales
// ==============================================================================

// ==============================================================================
// 14. Módulos Adicionales: Redes, Terminal, Aplicaciones, Dominio y Diagnósticos
// ==============================================================================
const NET_PORT_SERVICES = {
  22: 'sshd', 25: 'exim4', 53: 'dns', 80: 'nginx', 443: 'nginx',
  137: 'nmbd', 138: 'nmbd', 139: 'smbd', 445: 'smbd',
  3702: 'wsdd2', 5355: 'wsdd2',
};

function formatSmbProtocol(raw) {
  if (!raw) return 'N/D';
  const key = String(raw).toUpperCase();
  const map = {
    'SMB2_02': 'SMB 2.02',
    'SMB2_10': 'SMB 2.1',
    'SMB3_00': 'SMB 3.0',
    'SMB3_11': 'SMB 3.1.1',
    'SMB3': 'SMB 3.1.1 (máx.)',
  };
  return map[key] || key;
}

function netBadge(isOk, okLabel, failLabel) {
  return isOk
    ? `<span class="badge badge-ok">${escapeHtml(okLabel)}</span>`
    : `<span class="badge badge-err">${escapeHtml(failLabel)}</span>`;
}

function copyNetUnc(netbios) {
  const host = netbios || window.NAS_HOSTNAME || window.location.hostname || 'SRV-NAS';
  copyUncPath('\\\\' + host);
}

/**
 * Renderiza el inventario de red devuelto por GET /api/network.
 * Refleja exactamente la estructura de la vista server-side de layout.php.
 */
function renderNetworking(d) {
  if (!d || typeof d !== 'object') return;

  const samba = d.samba || {};
  const services = d.services || {};
  const firewall = d.firewall || {};
  const interfaces = Array.isArray(d.interfaces) ? d.interfaces : [];
  const dns = Array.isArray(d.dns) ? d.dns : [];
  const listening = Array.isArray(d.listening) ? d.listening : [];
  const netbios = d.netbios || d.hostname || window.location.hostname;
  const primaryIp = d.primary_ip || window.location.hostname;

  const masthead = document.getElementById('masthead-hostname');
  if (masthead && d.hostname) masthead.textContent = d.hostname;

  const setText = (id, value) => {
    const el = document.getElementById(id);
    if (el) el.textContent = value;
  };

  setText('net-netbios', netbios || 'N/D');
  setText('net-workgroup', d.workgroup || 'N/D');

  const ifaceEl = document.getElementById('net-interfaces');
  if (ifaceEl) {
    ifaceEl.innerHTML = interfaces.length === 0
      ? '<span class="text-muted">Sin interfaces detectadas.</span>'
      : interfaces.map(i => `
        <div class="net-iface">
          <strong>${escapeHtml(i.name || '')}</strong>
          <span class="badge ${i.state === 'UP' ? 'badge-ok' : 'badge-warn'}">${escapeHtml(i.state || 'N/D')}</span>
          <code>${escapeHtml(i.ip || 'sin IPv4')}${i.prefix ? '/' + Number(i.prefix) : ''}</code>
          <span class="text-muted">${i.speed_mbps ? Number(i.speed_mbps) + ' Mbps' : 'velocidad N/D'} · MTU ${Number(i.mtu || 0)}${i.mac ? ' · ' + escapeHtml(i.mac) : ''}</span>
        </div>
      `).join('');
  }

  const gatewayEl = document.getElementById('net-gateway');
  if (gatewayEl) {
    const mode = d.dhcp === null || d.dhcp === undefined
      ? ''
      : `<span class="badge ${d.dhcp ? 'badge-blue' : 'badge-gray'}">${d.dhcp ? 'DHCP' : 'Estática'}</span>`;
    gatewayEl.innerHTML = `<code>${escapeHtml(d.gateway || 'N/D')}</code> ${mode}`;
  }

  const dnsEl = document.getElementById('net-dns');
  if (dnsEl) {
    dnsEl.innerHTML = dns.length === 0
      ? '<span class="text-muted">N/D</span>'
      : escapeHtml(dns.join(' · '));
  }

  const smbEl = document.getElementById('net-smb');
  if (smbEl) {
    smbEl.innerHTML = `${escapeHtml(formatSmbProtocol(samba.min_protocol))} — ${escapeHtml(formatSmbProtocol(samba.max_protocol))}
      <span class="badge badge-gray">Firma: ${escapeHtml(samba.signing || 'N/D')}</span>
      <span class="badge badge-gray">Cifrado: ${escapeHtml(samba.encrypt || 'N/D')}</span>`;
  }

  const nmbdEl = document.getElementById('net-nmbd');
  if (nmbdEl) {
    nmbdEl.innerHTML = netBadge(Boolean(services.nmbd), 'nmbd activo', 'nmbd inactivo');
  }

  const wsdPorts = listening
    .filter(p => [3702, 5355].includes(Number(p.port)))
    .map(p => `${Number(p.port)}/${p.proto || 'udp'}`);
  const wsdEl = document.getElementById('net-wsd');
  if (wsdEl) {
    wsdEl.innerHTML = netBadge(Boolean(services.wsdd2), 'wsdd2 activo', 'wsdd2 inactivo') +
      (wsdPorts.length ? ` <span class="text-muted" style="font-size:12px;">puertos ${escapeHtml(wsdPorts.join(', '))}</span>` : '');
  }

  const fwEl = document.getElementById('net-firewall');
  if (fwEl) {
    fwEl.innerHTML = netBadge(Boolean(firewall.active), 'Activo', 'Inactivo') +
      (firewall.rules_visible === false ? ' <span class="text-muted" style="font-size:12px;">reglas no verificables sin privilegios</span>' : '');
  }

  const sessionsEl = document.getElementById('net-sessions');
  if (sessionsEl) {
    sessionsEl.innerHTML = (d.smb_sessions === null || d.smb_sessions === undefined)
      ? '<span class="text-muted">No verificable</span>'
      : `${Number(d.smb_sessions)} activa(s)`;
  }

  const pathEl = document.getElementById('net-info-path');
  if (pathEl) {
    pathEl.innerHTML = `<code>\\\\${escapeHtml(netbios)}</code> o <code>\\\\${escapeHtml(primaryIp)}</code>
      <button type="button" class="btn btn-secondary btn-sm" title="Copiar ruta UNC" style="margin-left:6px;" onclick="copyNetUnc('${escapeHtml(netbios)}')">
        <svg class="icon icon-sm"><use href="#icon-copy"></use></svg>
      </button>`;
  }

  const healthEl = document.getElementById('net-health-badge');
  if (healthEl) {
    const ok = Boolean(services.smbd) && Boolean(services.nmbd) && Boolean(services.wsdd2);
    healthEl.innerHTML = ok
      ? '<span class="badge badge-ok">Servicios de red activos</span>'
      : '<span class="badge badge-warn">Revisar servicios de red</span>';
  }

  const portsBody = document.getElementById('net-ports-body');
  if (portsBody) {
    portsBody.innerHTML = listening.length === 0
      ? '<span class="text-muted">Sin datos de puertos disponibles.</span>'
      : `<div class="net-port-list">${listening.map(p => {
        const svc = p.service || NET_PORT_SERVICES[Number(p.port)] || '';
        return `<span class="net-port-chip"><strong>${Number(p.port)}/${escapeHtml(p.proto || 'tcp')}</strong>${svc ? ' ' + escapeHtml(svc) : ''}</span>`;
      }).join('')}</div>`;
  }

  const portsCount = document.getElementById('net-ports-count');
  if (portsCount) portsCount.textContent = `${listening.length} puerto(s)`;

  const updatedEl = document.getElementById('net-updated');
  if (updatedEl) {
    updatedEl.textContent = d.generated_at ? `Actualizado ${d.generated_at}` : 'Sin verificar';
  }
}

async function loadNetworking(force = false) {
  try {
    const res = await apiFetch('/api/network' + (force ? '?fresh=1' : ''));
    renderNetworking(res.data || res);
  } catch (e) {
    // Manejado por apiFetch
  }
}

async function loadTerminal() {
  const input = document.getElementById('terminal-input');
  startTerminalSession();

  if (!AppState.terminal.history || AppState.terminal.history.length === 0) {
    try {
      const res = await apiFetch('/api/terminal/history?limit=50', { silentToast: true });
      const items = res.data ?? [];
      if (Array.isArray(items) && items.length > 0) {
        AppState.terminal.history = items.map(h => h.command).reverse();
        AppState.terminal.historyIndex = AppState.terminal.history.length;
      }
    } catch (e) {
      // Tolerante
    }
  }

  if (input) {
    setTimeout(() => input.focus(), 80);
  }
}


// ==============================================================================
