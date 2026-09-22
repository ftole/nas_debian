/*
 * EAD NAS - Cockpit Backups Plugin
 * JavaScript modular del módulo de copias de seguridad.
 */

var API_PATH = "/usr/share/cockpit/backups/backup_api.py";
var currentProto = "cifs";

/* =================== Utilidades =================== */

function esc(s) {
	return String(s == null ? "" : s)
		.replace(/&/g, "&amp;")
		.replace(/</g, "&lt;")
		.replace(/>/g, "&gt;")
		.replace(/"/g, "&quot;")
		.replace(/'/g, "&#39;");
}

/* =================== Tema Houston =================== */

function initTheme() {
	var state = localStorage.getItem("houston-theme-state");
	if (state === "light") {
		document.documentElement.setAttribute("data-theme", "light");
	} else {
		document.documentElement.setAttribute("data-theme", "dark");
	}
}

/* =================== API =================== */

function runApi(args) {
	return cockpit.spawn(["python3", API_PATH].concat(args), { superuser: "try", err: "message" })
		.then(function (out) {
			try {
				return JSON.parse(out.trim());
			} catch (e) {
				return { status: "error", message: "Respuesta inválida del servidor. " + String(e) };
			}
		})
		.catch(function (err) {
			var msg = "Error desconocido";
			if (err && err.message) msg = err.message;
			else if (err && err.problem) msg = err.problem;
			else msg = String(err);
			return { status: "error", message: "Error interno: " + msg };
		});
}

function runApiInput(action, payloadObj) {
	return new Promise(function (resolve) {
		var proc = cockpit.spawn(["python3", API_PATH, action], { superuser: "try", err: "message" });
		proc.input(JSON.stringify(payloadObj), true);
		proc.then(function (out) {
			try {
				resolve(JSON.parse(out.trim()));
			} catch (e) {
				resolve({ status: "error", message: "Respuesta inválida del servidor. " + String(e) });
			}
		});
		proc.catch(function (err) {
			var msg = "Error desconocido";
			if (err && err.message) msg = err.message;
			else if (err && err.problem) msg = err.problem;
			else msg = String(err);
			resolve({ status: "error", message: "Error interno: " + msg });
		});
	});
}

/* =================== Tabs =================== */

function switchTab(paneId, btnId) {
	var panes = document.querySelectorAll(".tab-pane");
	for (var i = 0; i < panes.length; i++) panes[i].style.display = "none";

	var btns = document.querySelectorAll(".bkp-tab-btn");
	for (var i = 0; i < btns.length; i++) btns[i].classList.remove("active");

	var pane = document.getElementById(paneId);
	var btn = document.getElementById(btnId);
	if (pane) pane.style.display = "block";
	if (btn) btn.classList.add("active");

	if (paneId === "tab-tasks") cargarTareas();
}

/* =================== Protocolo =================== */

function selectProto(proto) {
	currentProto = proto;
	var boxes = document.querySelectorAll(".bkp-proto-box");
	for (var i = 0; i < boxes.length; i++) boxes[i].classList.remove("selected");

	var card = document.getElementById("card-" + proto);
	if (card) card.classList.add("selected");

	var fields = document.querySelectorAll(".field-cifs, .field-ssh, .field-local, .field-remote");
	for (var i = 0; i < fields.length; i++) fields[i].style.display = "none";

	if (proto === "cifs") {
		show(".field-cifs");
		show(".field-remote");
	} else if (proto === "ssh") {
		show(".field-ssh");
		show(".field-remote");
	} else {
		show(".field-local");
	}
}

function show(selector) {
	var els = document.querySelectorAll(selector);
	for (var i = 0; i < els.length; i++) els[i].style.display = "flex";
}

/* =================== Tareas =================== */

function cargarTareas() {
	var tbody = document.getElementById("tasks-table-body");
	var logSelect = document.getElementById("log-task-select");
	if (!tbody) return;

	tbody.innerHTML = '<tr><td class="empty-msg" colspan="9"><i class="fas fa-spinner fa-spin"></i> Cargando tareas...</td></tr>';

	runApi(["list"]).then(function (res) {
		if (res.status !== "ok") {
			tbody.innerHTML = '<tr><td class="empty-msg" colspan="9">Error al cargar las tareas: ' + esc(res.message || "desconocido") + "</td></tr>";
			if (logSelect) logSelect.innerHTML = '<option value="">(Error)</option>';
			return;
		}
		if (!res.tasks || res.tasks.length === 0) {
			tbody.innerHTML = '<tr><td class="empty-msg" colspan="9">No hay tareas de backup programadas. Haz clic en "Nueva Tarea" para crear una.</td></tr>';
			if (logSelect) logSelect.innerHTML = '<option value="">(Sin tareas)</option>';
			return;
		}

		tbody.innerHTML = "";
		if (logSelect) logSelect.innerHTML = "";

		for (var i = 0; i < res.tasks.length; i++) {
			var t = res.tasks[i];

			if (logSelect) {
				var opt = document.createElement("option");
				opt.value = t.id;
				opt.textContent = t.id + " (" + t.proto + ")";
				logSelect.appendChild(opt);
			}

			var tr = document.createElement("tr");

			var badge = '<span class="bkp-badge bkp-badge-warn"><i class="fas fa-clock"></i> ' + esc(t.last_status) + "</span>";
			if (t.last_status === "Éxito") badge = '<span class="bkp-badge bkp-badge-ok"><i class="fas fa-check-circle"></i> Éxito</span>';
			if (t.last_status === "Fallo") badge = '<span class="bkp-badge bkp-badge-err"><i class="fas fa-times-circle"></i> Fallo</span>';

			tr.innerHTML =
				"<td><strong>" + esc(t.id) + "</strong></td>" +
				'<td><span class="bkp-badge bkp-badge-proto">' + esc(t.proto) + "</span></td>" +
				'<td><code style="color: #58a6ff;">' + esc(t.src) + "</code></td>" +
				"<td><code>" + esc(t.cron) + "</code></td>" +
				"<td>" + esc(t.retention) + " snaps</td>" +
				"<td><strong>" + esc(t.snaps) + "</strong> en disco</td>" +
				"<td>" + esc(t.last_run) + "</td>" +
				"<td>" + badge + "</td>" +
				'<td class="bkp-actions-row">' +
				'<button class="pf-c-button pf-m-secondary bkp-btn-exec" data-id="' + esc(t.id) + '"><i class="fas fa-bolt"></i></button>' +
				'<button class="pf-c-button pf-m-secondary bkp-btn-logs" data-id="' + esc(t.id) + '"><i class="fas fa-file-alt"></i></button>' +
				'<button class="pf-c-button pf-m-danger bkp-btn-del" data-id="' + esc(t.id) + '"><i class="fas fa-trash-alt"></i></button>' +
				"</td>";

			tbody.appendChild(tr);
		}

		// Attach listeners
		attachTableListeners();
	});
}

function attachTableListeners() {
	var execs = document.querySelectorAll(".bkp-btn-exec");
	for (var i = 0; i < execs.length; i++) {
		execs[i].addEventListener("click", function () { ejecutarAhora(this.getAttribute("data-id")); });
	}
	var logs = document.querySelectorAll(".bkp-btn-logs");
	for (var i = 0; i < logs.length; i++) {
		logs[i].addEventListener("click", function () { abrirModalLogs(this.getAttribute("data-id")); });
	}
	var dels = document.querySelectorAll(".bkp-btn-del");
	for (var i = 0; i < dels.length; i++) {
		dels[i].addEventListener("click", function () { eliminarTarea(this.getAttribute("data-id")); });
	}
}

/* =================== Conexión =================== */

function probarConexion() {
	var box = document.getElementById("alert-test");
	box.className = "bkp-alert bkp-alert-info visible";
	box.innerHTML = '<i class="fas fa-spinner fa-spin"></i> Probando conexión...';

	var payload = {
		ip: document.getElementById("task-ip").value.trim(),
		user: document.getElementById("task-user").value.trim(),
		password: document.getElementById("task-password").value
	};

	var action = "test_cifs";
	if (currentProto === "cifs") {
		payload.share = document.getElementById("task-share").value.trim();
	} else if (currentProto === "ssh") {
		action = "test_ssh";
		payload.port = document.getElementById("task-port").value.trim();
	}

	runApiInput(action, payload).then(function (res) {
		if (res.status === "ok") {
			box.className = "bkp-alert bkp-alert-ok visible";
			box.innerHTML = '<i class="fas fa-check-circle"></i> ' + esc(res.message);
		} else {
			box.className = "bkp-alert bkp-alert-err visible";
			box.innerHTML = '<i class="fas fa-exclamation-triangle"></i> ' + esc(res.message);
		}
	});
}

/* =================== Guardar =================== */

function guardarTarea() {
	var tid = document.getElementById("task-id").value.trim();
	if (!tid) { alert("Ingresa un identificador para la tarea."); return; }

	var cronVal = document.getElementById("task-cron-select").value;
	var cronExpr = (cronVal === "custom") ? document.getElementById("task-cron").value.trim() : cronVal;

	var payload = {
		id: tid,
		proto: currentProto,
		cron: cronExpr,
		retention: parseInt(document.getElementById("task-retention").value) || 30
	};

	if (currentProto === "cifs") {
		payload.ip = document.getElementById("task-ip").value.trim();
		payload.share = document.getElementById("task-share").value.trim();
		payload.user = document.getElementById("task-user").value.trim();
		payload.password = document.getElementById("task-password").value;
	} else if (currentProto === "ssh") {
		payload.ip = document.getElementById("task-ip").value.trim();
		payload.port = document.getElementById("task-port").value.trim();
		payload.path = document.getElementById("task-remote-path").value.trim();
		payload.user = document.getElementById("task-user").value.trim();
		payload.password = document.getElementById("task-password").value;
	} else {
		payload.path = document.getElementById("task-local-path").value.trim();
	}

	var btn = document.getElementById("btn-save");
	btn.innerHTML = '<i class="fas fa-spinner fa-spin"></i> Guardando...';

	runApiInput("create", payload).then(function (res) {
		btn.innerHTML = '<i class="fas fa-save"></i> Guardar y Programar';
		if (res.status === "ok") {
			alert("✔ " + res.message);
			switchTab("tab-tasks", "tab-btn-tasks");
		} else {
			alert("Error: " + res.message);
		}
	});
}

/* =================== Acciones =================== */

function ejecutarAhora(taskId) {
	if (!/^[A-Za-z0-9_-]+$/.test(taskId)) {
		alert("Identificador de tarea inválido.");
		return;
	}
	if (!confirm("¿Ejecutar backup [" + taskId + "] ahora en segundo plano?")) return;
	runApi(["run", taskId]).then(function (res) {
		alert(res.message || (res.status === "ok" ? "Tarea lanzada en segundo plano." : "Error al lanzar la tarea."));
		cargarTareas();
	});
}

function eliminarTarea(taskId) {
	if (!confirm("¿Eliminar tarea [" + taskId + "]? Los respaldos en disco se conservarán.")) return;
	runApi(["delete", taskId, "--confirm"]).then(function (res) {
		alert(res.message);
		cargarTareas();
	});
}

function abrirModalLogs(taskId) {
	document.getElementById("modal-log-title").textContent = "Registro: " + taskId;
	document.getElementById("modal-logs").classList.add("visible");
	document.getElementById("modal-log-console").textContent = "Cargando...";
	runApi(["logs", taskId]).then(function (res) {
		document.getElementById("modal-log-console").textContent = res.logs || "(Sin registros)";
	});
}

function verLogs(taskId) {
	if (!taskId) return;
	document.getElementById("log-console").textContent = "Cargando...";
	runApi(["logs", taskId]).then(function (res) {
		document.getElementById("log-console").textContent = res.logs || "(Sin registros)";
	});
}

function cerrarModal() {
	document.getElementById("modal-logs").classList.remove("visible");
}

/* =================== Init =================== */

document.addEventListener("DOMContentLoaded", function () {
	initTheme();

	document.getElementById("tab-btn-tasks").addEventListener("click", function () { switchTab("tab-tasks", "tab-btn-tasks"); });
	document.getElementById("tab-btn-new").addEventListener("click", function () { switchTab("tab-new", "tab-btn-new"); });
	document.getElementById("tab-btn-logs").addEventListener("click", function () { switchTab("tab-logs", "tab-btn-logs"); });

	document.getElementById("btn-refresh").addEventListener("click", cargarTareas);
	document.getElementById("btn-new-task-top").addEventListener("click", function () { switchTab("tab-new", "tab-btn-new"); });

	document.getElementById("card-cifs").addEventListener("click", function () { selectProto("cifs"); });
	document.getElementById("card-ssh").addEventListener("click", function () { selectProto("ssh"); });
	document.getElementById("card-local").addEventListener("click", function () { selectProto("local"); });

	document.getElementById("task-cron-select").addEventListener("change", function () {
		document.getElementById("group-custom-cron").style.display = (this.value === "custom") ? "flex" : "none";
	});

	document.getElementById("btn-test").addEventListener("click", probarConexion);
	document.getElementById("btn-save").addEventListener("click", guardarTarea);

	document.getElementById("btn-refresh-logs").addEventListener("click", function () {
		verLogs(document.getElementById("log-task-select").value);
	});
	document.getElementById("log-task-select").addEventListener("change", function () {
		verLogs(this.value);
	});

	document.getElementById("btn-modal-x").addEventListener("click", cerrarModal);
	document.getElementById("btn-close-modal").addEventListener("click", cerrarModal);

	cargarTareas();
});
