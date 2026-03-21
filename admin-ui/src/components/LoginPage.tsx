import { useState } from 'react';
import { useAuth } from '../lib/auth';
import api from '../lib/api';

export default function LoginPage() {
  const { setToken } = useAuth();
  const [inputToken, setInputToken] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  const handleLogin = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!inputToken.trim()) {
      setError('Please enter a token');
      return;
    }
    setLoading(true);
    setError('');
    // Temporarily set the token to test the API
    localStorage.setItem('icedb_admin_token', inputToken.trim());
    try {
      await api.get('/status');
      setToken(inputToken.trim());
    } catch (err: unknown) {
      localStorage.removeItem('icedb_admin_token');
      const status = (err as { response?: { status?: number } })?.response?.status;
      if (status === 401) {
        setError('Invalid token. Please try again.');
      } else {
        setError('Could not connect to icedb admin server. Is it running?');
      }
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="min-h-screen flex items-center justify-center bg-gray-50 dark:bg-gray-900">
      <div className="bg-white dark:bg-gray-800 rounded-xl shadow-lg p-8 w-full max-w-sm border border-gray-200 dark:border-gray-700">
        <div className="text-center mb-6">
          <div className="text-3xl font-bold text-blue-600 dark:text-blue-400 mb-1">icedb</div>
          <div className="text-sm text-gray-500 dark:text-gray-400">Admin UI</div>
        </div>
        <form onSubmit={handleLogin} className="space-y-4">
          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
              Admin Token
            </label>
            <input
              type="password"
              value={inputToken}
              onChange={(e) => setInputToken(e.target.value)}
              placeholder="Enter your admin token"
              className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:outline-none focus:ring-2 focus:ring-blue-500"
              autoFocus
            />
          </div>
          {error && (
            <div className="text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 px-3 py-2 rounded-lg">
              {error}
            </div>
          )}
          <button
            type="submit"
            disabled={loading}
            className="w-full py-2 px-4 bg-blue-600 hover:bg-blue-700 disabled:bg-blue-400 text-white font-medium rounded-lg transition-colors"
          >
            {loading ? 'Connecting...' : 'Login'}
          </button>
        </form>
        <p className="text-xs text-gray-400 dark:text-gray-500 text-center mt-4">
          Set ICEDB_ADMIN_TOKEN or use --admin-token when starting the server
        </p>
      </div>
    </div>
  );
}
