// Okno czatu Klaudiusza. Bez historii: wiadomości żyją tylko do zamknięcia aplikacji.
(() => {
  const THEME_KEY = 'klaudiusz.theme';
  const $ = (id) => document.getElementById(id);
  const chat = $('chat');
  const form = $('form');
  const input = $('input');
  const send = $('send');
  const settings = $('settings');
  const choices = document.querySelectorAll('[data-theme-choice]');

  const invoke = (cmd, args) => window.__TAURI__.core.invoke(cmd, args);

  // Motyw zapisujemy tylko w tej przeglądarce; bez zapisu idzie za systemem
  function storedTheme() {
    try {
      const value = localStorage.getItem(THEME_KEY);
      return value === 'light' || value === 'dark' ? value : null;
    } catch {
      return null;
    }
  }

  function currentTheme() {
    return storedTheme() ?? (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light');
  }

  function applyTheme(theme) {
    document.documentElement.dataset.theme = theme;
    choices.forEach((b) => b.setAttribute('aria-checked', String(b.dataset.themeChoice === theme)));
  }

  choices.forEach((b) =>
    b.addEventListener('click', () => {
      const theme = b.dataset.themeChoice;
      try {
        localStorage.setItem(THEME_KEY, theme);
      } catch {
        /* brak zapisu nie blokuje zmiany motywu */
      }
      applyTheme(theme);
    }),
  );

  function showSettings(open) {
    settings.hidden = !open;
    if (open) {
      applyTheme(currentTheme());
      $('back').focus();
    } else {
      $('gear').focus();
    }
  }

  $('gear').addEventListener('click', () => showSettings(true));
  $('back').addEventListener('click', () => showSettings(false));
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && !settings.hidden) showSettings(false);
  });

  // Treść tworzymy przez textContent: odpowiedź modelu to dane, nie HTML
  function addMessage(role, text) {
    $('hint')?.remove();
    const el = document.createElement('div');
    el.className = `msg ${role}`;
    el.textContent = text;
    chat.appendChild(el);
    chat.scrollTop = chat.scrollHeight;
    return el;
  }

  form.addEventListener('submit', async (e) => {
    e.preventDefault();
    const text = input.value.trim();
    if (!text) return;
    input.value = '';
    hideMenu();
    addMessage('user', text);
    const pending = addMessage('bot pending', '…');
    send.disabled = true;
    input.disabled = true;
    try {
      const answer = await invoke('chat_send', { text });
      pending.textContent = answer?.trim() ? answer : 'Nie mam odpowiedzi na to pytanie. Spróbuj przeformułować polecenie.';
      pending.classList.remove('pending');
    } catch (err) {
      pending.textContent = String(err);
      pending.classList.remove('pending');
      pending.classList.add('error');
    } finally {
      send.disabled = false;
      input.disabled = false;
      input.focus();
      chat.scrollTop = chat.scrollHeight;
    }
  });

  // Menu poleceń: po wpisaniu „/” lista narzędzi, dopasowana też do skróconej nazwy („/brie” → Daily_Brief)
  const menu = $('cmd-menu');
  let commands = [];
  let shown = [];
  let active = 0;

  // Listę pobieramy przy pierwszym „/”; poza Tauri (podgląd w przeglądarce) jej nie ma
  function loadCommands() {
    return invoke('chat_commands')
      .then((list) => {
        commands = list;
      })
      .catch(() => {});
  }

  // Porównanie bez wielkości liter, „_”, „-” i spacji
  const squash = (s) => s.toLowerCase().replace(/[\s_-]/g, '');

  // Im wyższy wynik, tym wyżej na liście: początek nazwy > fragment > kolejne litery > opis
  function score(query, cmd) {
    const q = squash(query);
    if (!q) return 1;
    const name = squash(cmd.name);
    if (name.startsWith(q)) return 4;
    if (name.includes(q)) return 3;
    let i = 0;
    for (const ch of name) {
      if (ch === q[i]) i++;
      if (i === q.length) return 2;
    }
    return squash(cmd.description).includes(q) ? 1 : 0;
  }

  function matches(query) {
    return commands
      .map((cmd) => ({ cmd, s: score(query, cmd) }))
      .filter((x) => x.s > 0)
      .sort((a, b) => b.s - a.s || a.cmd.name.localeCompare(b.cmd.name))
      .map((x) => x.cmd);
  }

  function hideMenu() {
    menu.hidden = true;
    shown = [];
  }

  function highlight() {
    [...menu.children].forEach((li, i) => li.setAttribute('aria-selected', String(i === active)));
    menu.children[active]?.scrollIntoView({ block: 'nearest' });
  }

  function renderMenu() {
    // menu działa tylko na pierwszym słowie: po spacji zaczynają się argumenty
    if (!input.value.startsWith('/') || input.value.includes(' ')) return hideMenu();
    shown = matches(input.value.slice(1));
    if (shown.length === 0) return hideMenu();
    menu.replaceChildren(
      ...shown.map((cmd, i) => {
        const li = document.createElement('li');
        li.className = 'cmd-item';
        li.setAttribute('role', 'option');
        li.setAttribute('aria-selected', String(i === active));
        li.dataset.index = String(i);
        const name = document.createElement('span');
        name.className = 'cmd-name';
        name.textContent = `/${cmd.name}`;
        const desc = document.createElement('span');
        desc.className = 'cmd-desc';
        desc.textContent = cmd.description;
        li.append(name, desc);
        return li;
      }),
    );
    menu.hidden = false;
  }

  function pick(index) {
    const cmd = shown[index];
    if (!cmd) return;
    input.value = `/${cmd.name} `;
    hideMenu();
    input.focus();
  }

  input.addEventListener('input', () => {
    active = 0;
    if (commands.length === 0 && input.value.startsWith('/')) {
      loadCommands().then(renderMenu);
    }
    renderMenu();
  });

  input.addEventListener('keydown', (e) => {
    if (menu.hidden || shown.length === 0) return;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      active = (active + 1) % shown.length;
      highlight();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      active = (active - 1 + shown.length) % shown.length;
      highlight();
    } else if (e.key === 'Enter' || e.key === 'Tab') {
      // pełna nazwa już wpisana: Enter wysyła, a nie wybiera ponownie
      if (e.key === 'Enter' && squash(input.value.slice(1)) === squash(shown[active].name)) return;
      e.preventDefault();
      pick(active);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      hideMenu();
    }
  });

  // Klik w pozycję: mousedown bez domyślnej akcji, żeby pole zachowało fokus
  menu.addEventListener('mousedown', (e) => {
    const li = e.target.closest('.cmd-item');
    if (!li) return;
    e.preventDefault();
    pick(Number(li.dataset.index));
  });

  input.addEventListener('blur', hideMenu);

  // Dyktowanie: rozpoznana mowa trafia do pola, a wysyła ją dopiero użytkownik
  const dictate = $('dictate');
  const voiceChat = $('voice-chat');

  dictate.addEventListener('click', async () => {
    dictate.classList.add('active');
    send.disabled = true;
    input.disabled = true;
    try {
      const text = await invoke('chat_dictate');
      if (text) input.value = text;
    } catch (err) {
      addMessage('error', String(err));
    } finally {
      dictate.classList.remove('active');
      send.disabled = false;
      input.disabled = false;
      input.focus();
    }
  });

  // Czat głosowy: włącza ciągłe nasłuchiwanie bez słowa aktywującego, drugie kliknięcie wyłącza
  function setVoice(on) {
    voiceChat.setAttribute('aria-pressed', String(on));
    dictate.disabled = on;
  }

  voiceChat.addEventListener('click', async () => {
    const on = voiceChat.getAttribute('aria-pressed') !== 'true';
    voiceChat.disabled = true;
    try {
      await invoke('chat_voice', { on });
      setVoice(on);
    } catch (err) {
      addMessage('error', String(err));
    } finally {
      voiceChat.disabled = false;
    }
  });

  // Wiadomości z czatu głosowego przychodzą z rdzenia jako zdarzenia
  // Bez Tauri (podgląd w przeglądarce) nasłuchiwania nie ma
  window.__TAURI__?.event.listen('chat-message', (event) => {
    const { role, text } = event.payload;
    addMessage(role === 'user' || role === 'error' ? role : 'bot', text);
  });

  applyTheme(currentTheme());
  input.focus();
})();
