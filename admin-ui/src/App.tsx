import { Navigate, Route, Routes } from 'react-router-dom';
import { useAuth } from './lib/auth';
import LoginPage from './components/LoginPage';
import Sidebar from './components/Sidebar';
import ServerStatusPage from './pages/ServerStatusPage';
import RolesPage from './pages/RolesPage';
import CreateRolePage from './pages/CreateRolePage';
import SchemasPage from './pages/SchemasPage';
import TableDetailPage from './pages/TableDetailPage';
import QueryConsolePage from './pages/QueryConsolePage';
import PermissionsPage from './pages/PermissionsPage';

function AuthGate({ children }: { children: React.ReactNode }) {
  const { token } = useAuth();
  if (!token) return <LoginPage />;
  return <>{children}</>;
}

export default function App() {
  return (
    <AuthGate>
      <div className="flex h-screen bg-gray-50 dark:bg-gray-950 overflow-hidden">
        <Sidebar />
        <main className="flex-1 overflow-y-auto p-6">
          <div className="max-w-5xl mx-auto h-full">
            <Routes>
              <Route path="/" element={<Navigate to="/status" replace />} />
              <Route path="/status" element={<ServerStatusPage />} />
              <Route path="/roles" element={<RolesPage />} />
              <Route path="/roles/new" element={<CreateRolePage />} />
              <Route path="/schemas" element={<SchemasPage />} />
              <Route path="/schemas/:schema/tables/:table" element={<TableDetailPage />} />
              <Route path="/query" element={<QueryConsolePage />} />
              <Route path="/permissions" element={<PermissionsPage />} />
              <Route path="*" element={<Navigate to="/status" replace />} />
            </Routes>
          </div>
        </main>
      </div>
    </AuthGate>
  );
}
