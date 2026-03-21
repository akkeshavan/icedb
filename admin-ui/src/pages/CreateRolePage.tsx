import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { ArrowLeft } from 'lucide-react';
import api from '../lib/api';
import { ToastContainer, useToast } from '../components/Toast';

interface CreateRoleBody {
  name: string;
  password: string;
  rolsuper: boolean;
  rolcreatedb: boolean;
  rolcreaterole: boolean;
  rolcanlogin: boolean;
}

export default function CreateRolePage() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { toasts, addToast, removeToast } = useToast();

  const [form, setForm] = useState<CreateRoleBody>({
    name: '',
    password: '',
    rolsuper: false,
    rolcreatedb: false,
    rolcreaterole: false,
    rolcanlogin: true,
  });

  const mutation = useMutation({
    mutationFn: (body: CreateRoleBody) => api.post('/roles', body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['roles'] });
      navigate('/roles');
    },
    onError: (err: unknown) => {
      const msg = (err as { response?: { data?: { error?: string } } })?.response?.data?.error ?? 'Unknown error';
      addToast('error', `Failed to create role: ${msg}`);
    },
  });

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!form.name.trim()) {
      addToast('error', 'Role name is required');
      return;
    }
    mutation.mutate(form);
  };

  const setCheck = (field: keyof CreateRoleBody, value: boolean) => {
    setForm((prev) => ({ ...prev, [field]: value }));
  };

  return (
    <div className="max-w-lg">
      <div className="flex items-center gap-3 mb-6">
        <button
          onClick={() => navigate('/roles')}
          className="p-2 rounded-lg text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors"
        >
          <ArrowLeft size={18} />
        </button>
        <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100">Create Role</h1>
      </div>

      <form
        onSubmit={handleSubmit}
        className="bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 p-6 space-y-5"
      >
        <div>
          <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
            Role Name <span className="text-red-500">*</span>
          </label>
          <input
            type="text"
            value={form.name}
            onChange={(e) => setForm((p) => ({ ...p, name: e.target.value }))}
            placeholder="e.g. readonly_user"
            className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:outline-none focus:ring-2 focus:ring-blue-500 font-mono"
          />
        </div>

        <div>
          <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
            Password
          </label>
          <input
            type="password"
            value={form.password}
            onChange={(e) => setForm((p) => ({ ...p, password: e.target.value }))}
            placeholder="Leave empty for no password"
            className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:outline-none focus:ring-2 focus:ring-blue-500"
          />
        </div>

        <div className="space-y-3">
          <p className="text-sm font-medium text-gray-700 dark:text-gray-300">Privileges</p>

          {(
            [
              { field: 'rolsuper', label: 'Superuser', desc: 'Bypasses all permission checks' },
              { field: 'rolcreatedb', label: 'Create Database', desc: 'Can create tables and indexes' },
              { field: 'rolcreaterole', label: 'Create Role', desc: 'Can create and manage roles' },
              { field: 'rolcanlogin', label: 'Can Login', desc: 'Can establish connections' },
            ] as { field: keyof CreateRoleBody; label: string; desc: string }[]
          ).map(({ field, label, desc }) => (
            <label
              key={field}
              className="flex items-start gap-3 p-3 rounded-lg border border-gray-200 dark:border-gray-600 hover:bg-gray-50 dark:hover:bg-gray-700 cursor-pointer"
            >
              <input
                type="checkbox"
                checked={form[field] as boolean}
                onChange={(e) => setCheck(field, e.target.checked)}
                className="mt-0.5"
              />
              <div>
                <p className="text-sm font-medium text-gray-800 dark:text-gray-200">{label}</p>
                <p className="text-xs text-gray-400 dark:text-gray-500">{desc}</p>
              </div>
            </label>
          ))}
        </div>

        <div className="flex gap-3 pt-2">
          <button
            type="button"
            onClick={() => navigate('/roles')}
            className="flex-1 py-2 text-sm font-medium text-gray-700 dark:text-gray-300 bg-white dark:bg-gray-700 border border-gray-300 dark:border-gray-600 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-600 transition-colors"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={mutation.isPending}
            className="flex-1 py-2 text-sm font-medium text-white bg-blue-600 hover:bg-blue-700 disabled:bg-blue-400 rounded-lg transition-colors"
          >
            {mutation.isPending ? 'Creating...' : 'Create Role'}
          </button>
        </div>
      </form>

      <ToastContainer toasts={toasts} removeToast={removeToast} />
    </div>
  );
}
