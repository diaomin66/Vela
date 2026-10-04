import React from 'react';
import ReactDOM from 'react-dom/client';
import '@fontsource-variable/manrope';
import '@fontsource-variable/noto-sans-sc';
import App from './App';
import './styles.css';
import { QueryClientProvider } from '@tanstack/react-query';
import { queryClient } from './lib/queryClient';
import { ThemeProvider } from './lib/theme';

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode><ThemeProvider><QueryClientProvider client={queryClient}><App /></QueryClientProvider></ThemeProvider></React.StrictMode>,
);
