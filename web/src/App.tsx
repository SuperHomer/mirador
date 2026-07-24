import Nav from "./components/Nav";
import Hero from "./components/Hero";
import Features from "./components/Features";
import CliShowcase from "./components/CliShowcase";
import Install from "./components/Install";
import Keybindings from "./components/Keybindings";
import Footer from "./components/Footer";
import { SHOW_SHORTCUTS } from "./config";

export default function App() {
  return (
    <div style={{ background: "var(--bg)", minHeight: "100vh" }}>
      <Nav />
      <div style={{ maxWidth: 1280, margin: "0 auto" }}>
      <Hero />
      <Features />
      <CliShowcase />
      <Install />
      {SHOW_SHORTCUTS && <Keybindings />}
      <Footer />
      </div>
    </div>
  );
}
