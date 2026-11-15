// The code blocks of the site, as the brand book draws them: Osso on Grafite, keywords and decorators in Volt,
// in both the light and the dark theme of the pages.
const VOLT = '#D7FF3F';
const OSSO = '#EDEBE4';
const SOFT = '#C9C8C1';
const NEVOA = '#9A9C96';
const GRAFITE = '#1B1D21';

export const codeTheme = {
  name: 'telvax-carbono',
  type: 'dark',
  colors: {
    'editor.background': GRAFITE,
    'editor.foreground': OSSO,
    'editor.selectionBackground': '#3A3D43',
    'editorLineNumber.foreground': NEVOA,
    'terminal.background': GRAFITE,
    'terminal.foreground': OSSO,
  },
  tokenColors: [
    { scope: ['comment', 'punctuation.definition.comment'], settings: { foreground: NEVOA, fontStyle: 'italic' } },
    { scope: ['keyword', 'storage', 'constant.language'], settings: { foreground: VOLT } },
    { scope: ['keyword.operator', 'punctuation'], settings: { foreground: OSSO } },
    {
      scope: ['entity.name.function.decorator', 'meta.attribute', 'punctuation.definition.attribute'],
      settings: { foreground: VOLT },
    },
    { scope: ['string', 'constant.character.escape'], settings: { foreground: SOFT } },
    { scope: ['meta.interpolation', 'punctuation.section.interpolation'], settings: { foreground: OSSO } },
    { scope: ['entity.name.type', 'support.type'], settings: { foreground: OSSO, fontStyle: 'bold' } },
    {
      scope: ['entity.name.function', 'support.function', 'support.module', 'constant.numeric', 'variable'],
      settings: { foreground: OSSO },
    },
  ],
};

export const codeStyle = {
  borderRadius: '0',
  borderColor: '#3A3D43',
  codeBackground: GRAFITE,
  codeFontFamily: 'var(--__sl-font-mono)',
  uiFontFamily: 'var(--__sl-font-mono)',
  frames: {
    shadowColor: 'transparent',
    editorTabBarBackground: '#0E0F11',
    editorActiveTabBackground: GRAFITE,
    editorActiveTabForeground: OSSO,
    editorActiveTabIndicatorTopColor: VOLT,
    editorActiveTabIndicatorBottomColor: 'transparent',
    editorTabBarBorderBottomColor: '#3A3D43',
    terminalBackground: GRAFITE,
    terminalTitlebarBackground: '#0E0F11',
    terminalTitlebarForeground: NEVOA,
    terminalTitlebarBorderBottomColor: '#3A3D43',
    terminalTitlebarDotsForeground: NEVOA,
    terminalTitlebarDotsOpacity: '1',
    inlineButtonForeground: OSSO,
    inlineButtonBorder: OSSO,
    tooltipSuccessBackground: VOLT,
    tooltipSuccessForeground: '#0E0F11',
  },
};
