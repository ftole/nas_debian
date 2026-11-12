function getCsrfToken() {
  const meta = document.querySelector('meta[name="csrf-token"]');
  return meta ? meta.getAttribute('content') : '';
}

async function apiFetch(endpoint, options = {}) {
  try {
    const method = (options.method || 'GET').toUpperCase();
    const headers = {
      'Accept': 'application/json',
      'Content-Type': 'application/json',
      ...(options.headers || {}),
    };

    if (['POST', 'PUT', 'PATCH', 'DELETE'].includes(method)) {
      const csrf = getCsrfToken();
      if (csrf) {
        headers['X-CSRF-Token'] = csrf;
      }
    }

    const res = await fetch(endpoint, {
      ...options,
      method,
      headers,
    });

    if (res.status === 401) {
      showToast('Sesión no autorizada o expirada. Redirigiendo...', 'warning');
      setTimeout(() => {
        window.location.href = '/login';
      }, 800);
      throw new Error('Sesión expirada.');
    }

    let data;
    try {
      data = await res.json();
    } catch (e) {
      throw new Error(`Error en respuesta del servidor (${res.status} ${res.statusText || 'Error'})`);
    }

    if (!res.ok || data.success === false) {
      const rawErr = data.error || data.message || data.output || `Error ${res.status}`;
      throw new Error(cleanErrorMessage(rawErr));
    }
    return data;
  } catch (err) {
    if (err.message !== 'Sesión expirada.' && !options.silentToast) {
      showToast(cleanErrorMessage(err.message), 'error');
    }
    throw err;
  }
}


async function logoutSession() {
  const btn = document.getElementById('btn-logout');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }
  try {
    await apiFetch('/logout', { method: 'POST' });
  } catch (e) {
    // Si la llamada falla o redirige
  } finally {
    window.location.href = '/login';
  }
}


