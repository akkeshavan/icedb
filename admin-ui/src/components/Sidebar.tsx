import { NavLink } from 'react-router-dom';
import {
  Server,
  Users,
  Database,
  Terminal,
  Shield,
  LogOut,
  Moon,
  Sun,
} from 'lucide-react';
import { useAuth } from '../lib/auth';
import { useState, useEffect } from 'react';

const navItems = [
  { to: '/status', icon: Server, label: 'Server Status' },
  { to: '/roles', icon: Users, label: 'Roles' },
  { to: '/schemas', icon: Database, label: 'Schemas & Tables' },
  { to: '/query', icon: Terminal, label: 'Query Console' },
  { to: '/permissions', icon: Shield, label: 'Permissions' },
];

export default function Sidebar() {
  const { logout } = useAuth();
  const [dark, setDark] = useState(() =>
    document.documentElement.classList.contains('dark')
  );

  useEffect(() => {
    if (dark) {
      document.documentElement.classList.add('dark');
    } else {
      document.documentElement.classList.remove('dark');
    }
  }, [dark]);

  return (
    <aside className="w-56 bg-white dark:bg-gray-900 border-r border-gray-200 dark:border-gray-700 flex flex-col h-full flex-shrink-0">
      {/* Logo */}
      <div className="px-4 py-5 border-b border-gray-200 dark:border-gray-700">
        <div className="text-xl font-bold text-blue-600 dark:text-blue-400">icedb</div>
        <div className="text-xs text-gray-400 dark:text-gray-500 mt-0.5">Admin UI</div>
      </div>

      {/* Navigation */}
      <nav className="flex-1 px-2 py-4 space-y-1">
        {navItems.map(({ to, icon: Icon, label }) => (
          <NavLink
            key={to}
            to={to}
            className={({ isActive }) =>
              `flex items-center gap-3 px-3 py-2 rounded-lg text-sm font-medium transition-colors ${
                isActive
                  ? 'bg-blue-50 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300'
                  : 'text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-800 hover:text-gray-900 dark:hover:text-gray-100'
              }`
            }
          >
            <Icon size={16} />
            {label}
          </NavLink>
        ))}
      </nav>

      {/* Footer */}
      <div className="px-2 py-4 border-t border-gray-200 dark:border-gray-700 space-y-1">
        <button
          onClick={() => setDark((d) => !d)}
          className="w-full flex items-center gap-3 px-3 py-2 rounded-lg text-sm text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-800 transition-colors"
        >
          {dark ? <Sun size={16} /> : <Moon size={16} />}
          {dark ? 'Light Mode' : 'Dark Mode'}
        </button>
        <button
          onClick={logout}
          className="w-full flex items-center gap-3 px-3 py-2 rounded-lg text-sm text-gray-600 dark:text-gray-400 hover:bg-red-50 dark:hover:bg-red-900/20 hover:text-red-600 dark:hover:text-red-400 transition-colors"
        >
          <LogOut size={16} />
          Logout
        </button>
      </div>
    </aside>
  );
}
