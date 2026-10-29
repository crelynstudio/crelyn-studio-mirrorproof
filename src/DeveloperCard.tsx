import { useState } from 'react';
import type { MouseEvent } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { ArrowUpRight, Globe, Mail } from 'lucide-react';
import { copy as t } from './copy';
import crelynLogo from './assets/CRELYN-DARK.png';
import github from './assets/platforms/github.svg';
import behance from './assets/platforms/behance.svg';
import instagram from './assets/platforms/instagram.svg';
import x from './assets/platforms/x.svg';
import threads from './assets/platforms/threads.svg';

const contacts = [
  { label: t.website, href: 'https://crelynstudio.com', Icon: Globe },
  { label: t.email, href: 'mailto:info@crelynstudio.com', Icon: Mail },
];
const platforms = [
  { label: 'GitHub', href: 'https://github.com/crelynstudio', icon: github },
  { label: 'Behance', href: 'https://www.behance.net/crelynstudio', icon: behance },
  { label: 'Instagram', href: 'https://www.instagram.com/crelynstudio/', icon: instagram },
  { label: 'X', href: 'https://x.com/crelynstudio', icon: x },
  { label: 'Threads', href: 'https://www.threads.com/@crelynstudio', icon: threads },
];

export default function DeveloperCard() {
  const [linkError, setLinkError] = useState(false);

  function followLink(event: MouseEvent<HTMLAnchorElement>, href: string) {
    event.preventDefault(); setLinkError(false);
    void openUrl(href).catch(() => setLinkError(true));
  }

  return <section className="developer-card" aria-label={t.aboutDeveloper}>
    <div className="developer-heading">
      <div className="developer-identity"><span>{t.developedBy}</span><img src={crelynLogo} alt="Crelyn Studio" /></div>
      <div className="developer-contact">{contacts.map(({ label, href, Icon }) => <a key={href} href={href} title={href} onClick={event => followLink(event, href)}><Icon size={14} aria-hidden="true" /><span>{label}</span><ArrowUpRight size={12} aria-hidden="true" /></a>)}</div>
    </div>
    <nav className="developer-socials" aria-label={t.developerLinks}>{platforms.map(({ label, href, icon }) => <a key={href} href={href} aria-label={label} title={label} onClick={event => followLink(event, href)}><img src={icon} alt="" aria-hidden="true" /></a>)}</nav>
    {linkError && <p className="developer-link-error" role="alert">{t.linkFailed}</p>}
  </section>;
}
