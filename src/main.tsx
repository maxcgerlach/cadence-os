import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import App from "./App";
import { LibraryProvider } from "./context/LibraryContext";

const queryClient = new QueryClient();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <LibraryProvider>
        <App />
      </LibraryProvider>
    </QueryClientProvider>
  </React.StrictMode>,
);
