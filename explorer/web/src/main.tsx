import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { createBrowserRouter, RouterProvider } from "react-router";
import { Layout } from "./components/Layout";
import "./index.css";
import { configured } from "./lib/supabase";
import { AccountPage, AccountsPage, MinersPage } from "./pages/Accounts";
import { AgentPage, AgentsPage } from "./pages/Agents";
import { BlockPage, BlocksPage } from "./pages/Blocks";
import { ChannelPage, ChannelsPage } from "./pages/Channels";
import { EventsPage, ExtrinsicPage, ExtrinsicsPage, TransfersPage } from "./pages/Extrinsics";
import { Home } from "./pages/Home";
import { NotConfigured, NotFound, SearchPage } from "./pages/Misc";
import { ShieldedPage, SupplyPage } from "./pages/Supply";
import { TaskPage, TasksPage } from "./pages/Tasks";

const queries = new QueryClient({
  defaultOptions: { queries: { staleTime: 10_000, retry: 1, refetchOnWindowFocus: false } },
});

const router = createBrowserRouter([
  {
    element: <Layout />,
    children: [
      { path: "/", element: <Home /> },
      { path: "/blocks", element: <BlocksPage /> },
      { path: "/block/:id", element: <BlockPage /> },
      { path: "/extrinsics", element: <ExtrinsicsPage /> },
      { path: "/extrinsic/:id", element: <ExtrinsicPage /> },
      { path: "/transfers", element: <TransfersPage /> },
      { path: "/events", element: <EventsPage /> },
      { path: "/accounts", element: <AccountsPage /> },
      { path: "/account/:address", element: <AccountPage /> },
      { path: "/miners", element: <MinersPage /> },
      { path: "/agents", element: <AgentsPage /> },
      { path: "/agent/:address", element: <AgentPage /> },
      { path: "/tasks", element: <TasksPage /> },
      { path: "/task/:id", element: <TaskPage /> },
      { path: "/channels", element: <ChannelsPage /> },
      { path: "/channel/:id", element: <ChannelPage /> },
      { path: "/supply", element: <SupplyPage /> },
      { path: "/shielded", element: <ShieldedPage /> },
      { path: "/search", element: <SearchPage /> },
      { path: "*", element: <NotFound /> },
    ],
  },
]);

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    {configured ? (
      <QueryClientProvider client={queries}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    ) : (
      <NotConfigured />
    )}
  </StrictMode>,
);
