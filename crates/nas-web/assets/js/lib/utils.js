function cleanErrorMessage(raw) {
  if (raw === null || raw === undefined) return '';
  let str = String(raw).trim();

  // Desempaquetar recursivamente strings JSON si vienen serializados
  for (let i = 0; i < 3; i++) {
    if (str.startsWith('{') && str.endsWith('}')) {
      try {
        const parsed = JSON.parse(str);
        if (parsed && typeof parsed === 'object') {
          const candidate = parsed.error || parsed.message || parsed.output;
          if (candidate && typeof candidate === 'string') {
            str = candidate.trim();
            continue;
          }
        }
      } catch (e) {
        // No es JSON válido
      }
    }
    break;
  }

  // Eliminar referencias técnicas a páginas de manual
  if (str.includes('Refer to the mount.cifs')) {
    str = str.split(/Refer to the mount\.cifs/i)[0].trim();
  }

  // Quitar prefijo de error interno si existe
  str = str.replace(/^operación denegada:\s*/i, '');

  // Mapear errores comunes de CIFS/mount a descripciones en español claro
  if (str.includes('mount error(13)') || (str.toLowerCase().includes('permission denied') && (str.toLowerCase().includes('cifs') || str.toLowerCase().includes('mount')))) {
    return 'Fallo de autenticación CIFS: Acceso denegado (usuario o contraseña incorrectos, o permisos insuficientes en el recurso compartido).';
  }
  if (str.includes('mount error(112)') || str.includes('mount error(113)') || str.toLowerCase().includes('host is down') || str.toLowerCase().includes('no route to host')) {
    return 'Fallo de conexión CIFS: El servidor remoto no responde o está apagado/fuera de red.';
  }
  if (str.includes('mount error(111)') || str.toLowerCase().includes('connection refused')) {
    return 'Fallo de conexión CIFS: Conexión rechazada (puerto CIFS/SMB 445 no disponible en el host).';
  }
  if (str.includes('mount error(6)') || str.includes('mount error(2)') || str.toLowerCase().includes('no such device')) {
    return 'Fallo de conexión CIFS: Recurso compartido no encontrado en el servidor remoto.';
  }
  if (str.includes('mount error(115)')) {
    return 'Fallo de conexión CIFS: Tiempo de espera agotado al conectar con el servidor remoto.';
  }

  return str;
}

function initServerClock(serverConfig) {
  if (!serverConfig) return;
  if (typeof serverConfig === 'object') {
    const initialTimestampSec = serverConfig.timestamp || Math.floor(Date.now() / 1000);
    _serverClock.timeDiffMs = (initialTimestampSec * 1000) - Date.now();
    if (serverConfig.timezone) _serverClock.tzIana = serverConfig.timezone;
    if (serverConfig.tz) _serverClock.tzAbbr = serverConfig.tz;
    if (typeof serverConfig.offset === 'number') _serverClock.tzOffsetSec = serverConfig.offset;
  } else {
    const initialTimestampSec = Number(serverConfig) || Math.floor(Date.now() / 1000);
    _serverClock.timeDiffMs = (initialTimestampSec * 1000) - Date.now();
    if (arguments[1]) _serverClock.tzAbbr = arguments[1];
  }

  try {
    if (_serverClock.tzIana) {
      _serverClock.formatter = new Intl.DateTimeFormat('en-GB', {
        timeZone: _serverClock.tzIana,
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit',
        hour12: false
      });
    }
  } catch (e) {
    _serverClock.formatter = null;
  }

  updateServerClockDisplay();
  if (!_serverClock.timer) {
    _serverClock.timer = setInterval(updateServerClockDisplay, 1000);
  }
}


function updateServerClockDisplay(newTzAbbr) {
  const clockEl = document.getElementById('header-server-clock');
  if (!clockEl) return;

  if (newTzAbbr && typeof newTzAbbr === 'string') {
    _serverClock.tzAbbr = newTzAbbr;
  }

  let timeStr = '';
  const nowServerMs = Date.now() + _serverClock.timeDiffMs;

  if (_serverClock.formatter) {
    try {
      timeStr = _serverClock.formatter.format(new Date(nowServerMs));
    } catch (e) {
      timeStr = '';
    }
  }

  if (!timeStr) {
    const serverDate = new Date(nowServerMs + (_serverClock.tzOffsetSec * 1000));
    const pad = n => String(n).padStart(2, '0');
    timeStr = `${pad(serverDate.getUTCHours())}:${pad(serverDate.getUTCMinutes())}:${pad(serverDate.getUTCSeconds())}`;
  }

  clockEl.textContent = timeStr;

  const tzEl = document.getElementById('header-server-tz');
  if (tzEl && _serverClock.tzAbbr) {
    tzEl.textContent = _serverClock.tzAbbr;
  }

  const pillEl = document.getElementById('header-clock-pill');
  if (pillEl) {
    pillEl.title = `Hora local del servidor NAS: ${timeStr} ${_serverClock.tzAbbr} (${_serverClock.tzIana || 'Local'})`;
  }
}


function toggleTheme() {
  const html = document.documentElement;
  const current = html.getAttribute('data-theme') || 'dark';
  const next = current === 'dark' ? 'light' : 'dark';
  html.setAttribute('data-theme', next);

  try {
    localStorage.setItem('nas_theme', next);
  } catch (e) {
    console.warn('No se pudo persistir la preferencia de tema en localStorage:', e);
  }
}


function escapeHtml(str) {
  if (typeof str !== 'string') return String(str ?? '');
  return str.replace(/[&<>'"]/g,
    tag => ({
      '&': '&amp;',
      '<': '&lt;',
      '>': '&gt;',
      "'": '&#39;',
      '"': '&quot;'
    }[tag] || tag)
  );
}

