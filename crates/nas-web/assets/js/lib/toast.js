function showToast(message, type = 'info') {
  const container = document.getElementById('toast-container');
  if (!container) return;

  const toast = document.createElement('div');
  toast.className = `alert-box alert-box-${type === 'error' ? 'danger' : (type === 'success' ? 'success' : 'info')}`;
  toast.style.boxShadow = '0 4px 12px rgba(0,0,0,0.3)';
  toast.style.minWidth = '280px';
  toast.style.transition = 'opacity 0.3s ease';

  const msgDiv = document.createElement('div');
  msgDiv.textContent = type === 'error' ? cleanErrorMessage(message) : String(message ?? '');
  toast.appendChild(msgDiv);
  container.appendChild(toast);

  setTimeout(() => {
    toast.style.opacity = '0';
    setTimeout(() => toast.remove(), 300);
  }, 4000);
}

// Reloj del Servidor en Vivo Sincronizado
const _serverClock = {
  timeDiffMs: 0,
  tzIana: 'America/Mexico_City',
  tzAbbr: 'CST',
  tzOffsetSec: -21600,
  formatter: null,
  timer: null
};


