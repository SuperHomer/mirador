import logoUrl from "../assets/mira-logo.svg";

export default function Logo({ size = 32 }: { size?: number }) {
  return <img src={logoUrl} alt="Mirador" width={size} height={size} style={{ display: "block" }} />;
}
