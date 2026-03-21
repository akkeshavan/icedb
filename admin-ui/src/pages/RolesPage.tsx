import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { Plus, Trash2 } from 'lucide-react';
import api from '../lib/api';
import Badge from '../components/Badge';
import ConfirmDialog from '../components/ConfirmDialog';
import { ToastContainer, useToast } from '../components/Toast';
import { useState } from 'react';

interface Role {
  oid: number;
  rolname: string;
  rolsuper: boolean;
  rolinherit: boolean;
  rolcreaterole: boolean;
  rolcreatedb: boolean;
  rolcanlogin: boolean;
  rolbypassrls: boolean;
  has_password: boolean;
}

export default function RolesPage() {
  const queryClient = useQueryClient();
  const { toasts, addToast, removeToast } = useToast();
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);

  const { data, isLoading, error } = useQuery<{ roles: Role[] }>({
    queryKey: ['roles'],
    queryFn: () => api.get('/roles').then((r) => r.data),
  });

  const deleteMutation = useMutation({
    mutationFn: (name: string) => api.delete(`/roles/${name}`),
    onSuccess: (_, name) => {
      queryClient.invalidateQueries({ queryKey: ['roles'] });
      addToast('success', `Role '${name}' deleted`);
      setDeleteTarget(null);
    },
    onError: (err: unknown, name) => {
      const msg = (err as { response?: { data?: { error?: string } } })?.response?.data?.error ?? 'Unknown error';
      addToast('error', `Failed to delete '${name}': ${msg}`);
      setDeleteTarget(null);
    },
  });

  if (isLoading) {
    return <div className="text-gray-500 dark:text-gray-400">Loading roles...</div>;
  }

  if (error) {
    return (
      <div className="bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-700 rounded-xl p-4 text-red-700 dark:text-red-400">
        Failed to load roles.
      </div>
    );
  }

  const roles = data?.roles ?? [];

  return (
    <div>
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100">Roles</h1>
        <Link
          to="/roles/new"
          className="flex items-center gap-2 px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white text-sm font-medium rounded-lg transition-colors"
        >
          <Plus size={16} />
          New Role
        </Link>
      </div>

      <div className="bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 overflow-hidden">
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-900/50">
              <th className="px-4 py-3 text-left font-medium text-gray-600 dark:text-gray-400">Name</th>
              <th className="px-4 py-3 text-left font-medium text-gray-600 dark:text-gray-400">Superuser</th>
              <th className="px-4 py-3 text-left font-medium text-gray-600 dark:text-gray-400">CreateDB</th>
              <th className="px-4 py-3 text-left font-medium text-gray-600 dark:text-gray-400">CreateRole</th>
              <th className="px-4 py-3 text-left font-medium text-gray-600 dark:text-gray-400">CanLogin</th>
              <th className="px-4 py-3 text-left font-medium text-gray-600 dark:text-gray-400">Password</th>
              <th className="px-4 py-3 text-right font-medium text-gray-600 dark:text-gray-400">Actions</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-100 dark:divide-gray-700">
            {roles.length === 0 ? (
              <tr>
                <td colSpan={7} className="px-4 py-8 text-center text-gray-400 dark:text-gray-500">
                  No roles found.
                </td>
              </tr>
            ) : (
              roles.map((role) => (
                <tr key={role.oid ?? role.rolname} className="hover:bg-gray-50 dark:hover:bg-gray-700/50">
                  <td className="px-4 py-3 font-mono text-gray-900 dark:text-gray-100 font-medium">
                    {role.rolname}
                  </td>
                  <td className="px-4 py-3"><Badge value={role.rolsuper} /></td>
                  <td className="px-4 py-3"><Badge value={role.rolcreatedb} /></td>
                  <td className="px-4 py-3"><Badge value={role.rolcreaterole} /></td>
                  <td className="px-4 py-3"><Badge value={role.rolcanlogin} /></td>
                  <td className="px-4 py-3"><Badge value={role.has_password} trueLabel="Set" falseLabel="None" /></td>
                  <td className="px-4 py-3 text-right">
                    <button
                      onClick={() => setDeleteTarget(role.rolname)}
                      className="p-1.5 text-gray-400 hover:text-red-600 dark:hover:text-red-400 hover:bg-red-50 dark:hover:bg-red-900/20 rounded transition-colors"
                      title="Delete role"
                    >
                      <Trash2 size={15} />
                    </button>
                  </td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>

      <ConfirmDialog
        isOpen={deleteTarget !== null}
        title="Delete Role"
        message={`Are you sure you want to delete role '${deleteTarget}'? This action cannot be undone.`}
        confirmText="Delete"
        onConfirm={() => deleteTarget && deleteMutation.mutate(deleteTarget)}
        onCancel={() => setDeleteTarget(null)}
      />

      <ToastContainer toasts={toasts} removeToast={removeToast} />
    </div>
  );
}
