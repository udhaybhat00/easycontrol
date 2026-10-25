import logo from "./assets/easycontrol-logo.png";

const quote = "Secure access. Clear control.";

export function SplashScreen() {
  return <main className="splash" aria-label="EasyControl is loading"><div className="splash-orbit"><span/><span/><span/></div><section className="splash-card"><img src={logo} alt="EasyControl"/><div className="splash-wordmark">EasyControl</div><p>{quote}</p><div className="splash-progress"><i/></div><small>Preparing your secure workspace</small></section></main>;
}
