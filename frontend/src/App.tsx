import { Shell } from "./layouts/Shell";
import { StoreProvider } from "./store";

export default function App() {
  return (
    <StoreProvider>
      <Shell />
    </StoreProvider>
  );
}
