import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { useParams, useNavigate } from 'react-router-dom';
import { ArrowLeft, Trash2, Eraser } from 'lucide-react';
import api from '../lib/api';
import ConfirmDialog from '../components/ConfirmDialog';
import { ToastContainer, useToast } from '../components/Toast';
import { useState } from 'react';

interface ColumnDef {
  name: string;
  type: string;
  not_null: boolean;
  has_default: boolean;
  attnum: number;
}

interface TableSchemaData {
  oid: number;
  name: string;
  schema: string;
  columns: ColumnDef[];
}

interface IndexInfo {
  column: string;
  type: string;
  path: string;
}

interface IndexData {
  indexes: IndexInfo[];
}

interface RowCountData {
  row_count: string;
}

export default function TableDetailPage() {
  const { schema = 'public', table = '' } = useParams<{ schema: string; table: string }>();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { toasts, addToast, removeToast } = useToast();
  const [showDrop, setShowDrop] = useState(false);
  const [showTruncate, setShowTruncate] = useState(false);

  const schemaQuery = useQuery<TableSchemaData>({
    queryKey: ['table', schema, table, 'schema'],
    queryFn: () => api.get(`/tables/${schema}/${table}`).then((r) => r.data),
  });

  const indexQuery = useQuery<IndexData>({
    queryKey: ['table', schema, table, 'indexes'],
    queryFn: () => api.get(`/tables/${schema}/${table}/indexes`).then((r) => r.data),
  });

  const rowCountQuery = useQuery<RowCountData>({
    queryKey: ['table', schema, table, 'rowcount'],
    queryFn: () => api.get(`/tables/${schema}/${table}/rowcount`).then((r) => r.data),
  });

  const dropMutation = useMutation({
    mutationFn: () => api.delete(`/tables/${schema}/${table}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['schemas'] });
      addToast('success', `Table '${table}' dropped`);
      navigate(`/schemas`);
    },
    onError: (err: unknown) => {
      const msg = (err as { response?: { data?: { error?: string } } })?.response?.data?.error ?? 'Unknown error';
      addToast('error', `Failed to drop table: ${msg}`);
      setShowDrop(false);
    },
  });

  const truncateMutation = useMutation({
    mutationFn: () => api.post(`/tables/${schema}/${table}/truncate`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['table', schema, table] });
      addToast('success', `Table '${table}' truncated`);
      setShowTruncate(false);
    },
    onError: (err: unknown) => {
      const msg = (err as { response?: { data?: { error?: string } } })?.response?.data?.error ?? 'Unknown error';
      addToast('error', `Failed to truncate table: ${msg}`);
      setShowTruncate(false);
    },
  });

  return (
    <div>
      <div className="flex items-center justify-between mb-6">
        <div className="flex items-center gap-3">
          <button
            onClick={() => navigate('/schemas')}
            className="p-2 rounded-lg text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors"
          >
            <ArrowLeft size={18} />
          </button>
          <div>
            <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100 font-mono">{table}</h1>
            <p className="text-xs text-gray-400 dark:text-gray-500">schema: {schema}</p>
          </div>
        </div>
        <div className="flex gap-2">
          <button
            onClick={() => setShowTruncate(true)}
            className="flex items-center gap-2 px-3 py-2 text-sm font-medium text-orange-600 dark:text-orange-400 bg-orange-50 dark:bg-orange-900/20 border border-orange-200 dark:border-orange-700 rounded-lg hover:bg-orange-100 dark:hover:bg-orange-900/30 transition-colors"
          >
            <Eraser size={14} />
            Truncate
          </button>
          <button
            onClick={() => setShowDrop(true)}
            className="flex items-center gap-2 px-3 py-2 text-sm font-medium text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-700 rounded-lg hover:bg-red-100 dark:hover:bg-red-900/30 transition-colors"
          >
            <Trash2 size={14} />
            Drop Table
          </button>
        </div>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-4">
        {/* Columns */}
        <div className="lg:col-span-2 bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 overflow-hidden">
          <div className="px-5 py-4 border-b border-gray-200 dark:border-gray-700">
            <h2 className="font-semibold text-gray-900 dark:text-gray-100">Columns</h2>
          </div>
          {schemaQuery.isLoading ? (
            <div className="px-5 py-4 text-sm text-gray-400">Loading...</div>
          ) : (
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-gray-100 dark:border-gray-700 bg-gray-50 dark:bg-gray-900/50">
                  <th className="px-4 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400">#</th>
                  <th className="px-4 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400">Name</th>
                  <th className="px-4 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400">Type</th>
                  <th className="px-4 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400">Not Null</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-100 dark:divide-gray-700">
                {schemaQuery.data?.columns.map((col) => (
                  <tr key={col.name} className="hover:bg-gray-50 dark:hover:bg-gray-700/30">
                    <td className="px-4 py-2 text-xs text-gray-400 dark:text-gray-500">{col.attnum}</td>
                    <td className="px-4 py-2 font-mono text-gray-900 dark:text-gray-100">{col.name}</td>
                    <td className="px-4 py-2 font-mono text-xs text-blue-600 dark:text-blue-400">{col.type}</td>
                    <td className="px-4 py-2 text-xs text-gray-500 dark:text-gray-400">{col.not_null ? 'NOT NULL' : ''}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>

        {/* Right column: row count + indexes */}
        <div className="space-y-4">
          {/* Row count */}
          <div className="bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 p-5">
            <h2 className="font-semibold text-gray-900 dark:text-gray-100 mb-2">Row Count</h2>
            {rowCountQuery.isLoading ? (
              <div className="text-sm text-gray-400">Counting...</div>
            ) : (
              <div className="text-3xl font-bold text-gray-900 dark:text-gray-100">
                {rowCountQuery.data?.row_count ?? '?'}
              </div>
            )}
          </div>

          {/* Indexes */}
          <div className="bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 p-5">
            <h2 className="font-semibold text-gray-900 dark:text-gray-100 mb-3">Indexes</h2>
            {indexQuery.isLoading ? (
              <div className="text-sm text-gray-400">Loading...</div>
            ) : !indexQuery.data?.indexes.length ? (
              <div className="text-sm text-gray-400 dark:text-gray-500">No indexes registered.</div>
            ) : (
              <ul className="space-y-2">
                {indexQuery.data.indexes.map((idx) => (
                  <li key={idx.column} className="text-sm">
                    <span className="font-mono text-gray-900 dark:text-gray-100">{idx.column}</span>
                    <span className="ml-2 text-xs text-blue-600 dark:text-blue-400">{idx.type}</span>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>
      </div>

      <ConfirmDialog
        isOpen={showDrop}
        title="Drop Table"
        message={`This will permanently drop '${table}' and all its data. This action cannot be undone.`}
        confirmText="Drop Table"
        requireTyping={table}
        onConfirm={() => dropMutation.mutate()}
        onCancel={() => setShowDrop(false)}
      />

      <ConfirmDialog
        isOpen={showTruncate}
        title="Truncate Table"
        message={`This will delete all rows from '${table}'. The table structure will remain. This cannot be undone.`}
        confirmText="Truncate"
        onConfirm={() => truncateMutation.mutate()}
        onCancel={() => setShowTruncate(false)}
      />

      <ToastContainer toasts={toasts} removeToast={removeToast} />
    </div>
  );
}
