(() => {
  let mode = 'system';
  try {
    const saved = localStorage.getItem('vela:appearance:v1');
    if (saved === 'light' || saved === 'dark') mode = saved;
  } catch {}
  const theme = mode === 'system' ? matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light' : mode;
  document.documentElement.dataset.themeMode = mode;
  document.documentElement.dataset.theme = theme;
  document.documentElement.style.colorScheme = theme;
  document.querySelector('meta[name="theme-color"]')?.setAttribute('content', theme === 'dark' ? '#14171d' : '#f3f4f6');
})();
