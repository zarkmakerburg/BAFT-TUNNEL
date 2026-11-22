const ICONS: Record<string, string[]> = {
  overview: ['M3 3h7v9H3z', 'M14 3h7v5h-7z', 'M14 12h7v9h-7z', 'M3 16h7v5H3z'],
  tunnels: ['M3 8h14', 'M13 4l4 4-4 4', 'M21 16H7', 'M11 12l-4 4 4 4'],
  fleet: ['M12 3l8 4.5v9L12 21l-8-4.5v-9z', 'M12 12l8-4.5', 'M12 12v9', 'M12 12L4 7.5'],
  servers: ['M3 4h18v7H3z', 'M3 13h18v7H3z', 'M7 7.5h.01', 'M7 16.5h.01', 'M11 7.5h6', 'M11 16.5h6'],
  monitoring: ['M3 3v18h18', 'M7 15l4-5 3 3 5-7'],
  setup: ['M12 3v4', 'M12 17v4', 'M3 12h4', 'M17 12h4', 'M12 8a4 4 0 100 8 4 4 0 000-8z'],
  diagnostics: ['M3 12h4l2-6 4 12 2-6h6'],
  activity: ['M8 6h13', 'M8 12h13', 'M8 18h13', 'M3.5 6h.01', 'M3.5 12h.01', 'M3.5 18h.01'],
  agent: ['M7 7h10v10H7z', 'M10 10h4v4h-4z', 'M9 3v4', 'M15 3v4', 'M9 17v4', 'M15 17v4', 'M3 9h4', 'M3 15h4', 'M17 9h4', 'M17 15h4'],
  settings: ['M4 6h10', 'M18 6h2', 'M4 12h2', 'M10 12h10', 'M4 18h10', 'M18 18h2', 'M16 4v4', 'M8 10v4', 'M16 16v4'],
  menu: ['M4 6h16', 'M4 12h16', 'M4 18h16'],
  close: ['M6 6l12 12', 'M18 6L6 18'],
  search: ['M11 4a7 7 0 100 14 7 7 0 000-14z', 'M20 20l-3.5-3.5'],
  bell: ['M6 9a6 6 0 0112 0c0 6 2 7 2 8H4c0-1 2-2 2-8z', 'M10 20a2 2 0 004 0'],
  sun: ['M12 8a4 4 0 100 8 4 4 0 000-8z', 'M12 2v2', 'M12 20v2', 'M2 12h2', 'M20 12h2', 'M5 5l1.5 1.5', 'M17.5 17.5L19 19', 'M19 5l-1.5 1.5', 'M6.5 17.5L5 19'],
  moon: ['M20 14.5A8 8 0 019.5 4 8 8 0 1020 14.5z'],
  globe: ['M12 3a9 9 0 100 18 9 9 0 000-18z', 'M3 12h18', 'M12 3c3 3 3 15 0 18', 'M12 3c-3 3-3 15 0 18'],
  panel: ['M3 4h18v16H3z', 'M9 4v16'],
  logout: ['M9 4H4v16h5', 'M14 8l4 4-4 4', 'M18 12H9'],
  refresh: ['M20 11a8 8 0 00-14.5-4.5L4 8', 'M4 4v4h4', 'M4 13a8 8 0 0014.5 4.5L20 16', 'M20 20v-4h-4'],
  chevron: ['M9 6l6 6-6 6'],
  plus: ['M12 5v14', 'M5 12h14'],
  more: ['M5 12h.01', 'M12 12h.01', 'M19 12h.01'],
};

export function Icon({ name }: { name: string }) {
  return (
    <svg className="ic" viewBox="0 0 24 24" aria-hidden="true">
      {(ICONS[name] ?? []).map((d) => <path key={d} d={d} />)}
    </svg>
  );
}

/** Brand mark: the same mesh the overview draws, reduced to an outline that survives 30 px. */
export function Mark() {
  return (
    <svg className="brand-mark" viewBox="0 0 32 32" aria-hidden="true">
      <g className="e">
        <path d="M7 11.5 L16 6.5 L25 11.5 L25 20.5 L16 25.5 L7 20.5 Z" />
        <path d="M7 11.5 L25 20.5 M25 11.5 L16 25.5 M16 6.5 L7 20.5" />
      </g>
      <circle className="n" cx="16" cy="6.5" r="1.9" />
      <circle className="n" cx="25" cy="11.5" r="1.3" />
      <circle className="n" cx="25" cy="20.5" r="1.3" />
      <circle className="n" cx="16" cy="25.5" r="1.3" />
      <circle className="n" cx="7" cy="20.5" r="1.3" />
      <circle className="w" cx="7" cy="11.5" r="2.1" />
    </svg>
  );
}
