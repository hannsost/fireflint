import { Navigate, Route, Routes } from "react-router-dom";
import { useAuth } from "./auth/AuthContext";
import { Layout } from "./components/Layout";
import { LoginPage } from "./pages/LoginPage";
import { ContentListPage } from "./pages/ContentListPage";
import { ContentEditPage } from "./pages/ContentEditPage";
import { WebsitesPage } from "./pages/WebsitesPage";
import { TokensPage } from "./pages/TokensPage";

export default function App() {
  const { user, loading } = useAuth();

  if (loading) {
    return <div className="login-wrap muted">Lädt…</div>;
  }

  if (!user) {
    return (
      <Routes>
        <Route path="/login" element={<LoginPage />} />
        <Route path="*" element={<Navigate to="/login" replace />} />
      </Routes>
    );
  }

  return (
    <Routes>
      <Route path="/login" element={<Navigate to="/" replace />} />
      <Route element={<Layout />}>
        <Route index element={<Navigate to="/content/job" replace />} />
        <Route path="/content/:typeKey" element={<ContentListPage />} />
        <Route path="/content/:typeKey/new" element={<ContentEditPage />} />
        <Route path="/content/:typeKey/:id" element={<ContentEditPage />} />
        <Route path="/websites" element={<WebsitesPage />} />
        <Route path="/tokens" element={<TokensPage />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Route>
    </Routes>
  );
}
