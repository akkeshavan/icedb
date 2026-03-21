import { useQuery } from '@tanstack/react-query';
import { Shield } from 'lucide-react';
import api from '../lib/api';

interface PermissionsData {
  note: string;
}

export default function PermissionsPage() {
  const { data } = useQuery<PermissionsData>({
    queryKey: ['permissions'],
    queryFn: () => api.get('/permissions').then((r) => r.data),
  });

  return (
    <div>
      <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100 mb-6">Permissions</h1>
      <div className="bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 p-6">
        <div className="flex items-start gap-4">
          <div className="w-10 h-10 rounded-lg bg-yellow-50 dark:bg-yellow-900/30 flex items-center justify-center flex-shrink-0">
            <Shield size={20} className="text-yellow-600 dark:text-yellow-400" />
          </div>
          <div>
            <h2 className="font-semibold text-gray-900 dark:text-gray-100 mb-2">
              Table-Level Permissions
            </h2>
            <p className="text-sm text-gray-600 dark:text-gray-400">
              {data?.note ?? 'Loading...'}
            </p>
            <div className="mt-4 text-sm text-gray-500 dark:text-gray-400">
              <p>
                Role-level privileges (SUPERUSER, CREATEROLE, CREATEDB, LOGIN) can be managed in the{' '}
                <a href="/roles" className="text-blue-600 dark:text-blue-400 hover:underline">
                  Roles
                </a>{' '}
                section.
              </p>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
