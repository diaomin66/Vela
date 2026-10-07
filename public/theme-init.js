(() => {
  let mode = 'system';
  try {
    const key = 'ahax:appearance:v1';
    const legacyKey = 'vela:appearance:v1';
    const current = localStorage.getItem(key);
    const saved = current ?? localStorage.getItem(legacyKey);
    if (saved === 'light' || saved === 'dark' || saved === 'system') {
      mode = saved;
      if (current === null) localStorage.setItem(key, saved);
      localStorage.removeItem(legacyKey);
    }
  } catch {}
  const theme = mode === 'system' ? matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light' : mode;
  document.documentElement.dataset.themeMode = mode;
  document.documentElement.dataset.theme = theme;
  document.documentElement.style.colorScheme = theme;
  document.querySelector('meta[name="theme-color"]')?.setAttribute('content', theme === 'dark' ? '#11151c' : '#f7f8fa');
})();
