import Nav from "./components/Nav";
import Hero from "./components/Hero";
import Features from "./components/Features";
import AgentShowcase from "./components/AgentShowcase";
import DiffShowcase from "./components/DiffShowcase";
import GraphShowcase from "./components/GraphShowcase";
import PersistShowcase from "./components/PersistShowcase";
import CliShowcase from "./components/CliShowcase";
import Install from "./components/Install";
import Keybindings from "./components/Keybindings";
import Contribute from "./components/Contribute";
import Footer from "./components/Footer";
import { SHOW_SHORTCUTS } from "./config";

export default function App() {
  return (
    <div style={{ background: "var(--bg)", minHeight: "100vh" }}>
      <Nav />
      <Hero />
      <Features />
      <AgentShowcase />
      <DiffShowcase />
      <GraphShowcase />
      <PersistShowcase />
      <CliShowcase />
      <Install />
      {SHOW_SHORTCUTS && <Keybindings />}
      <Contribute />
      <Footer />
    </div>
  );
}
