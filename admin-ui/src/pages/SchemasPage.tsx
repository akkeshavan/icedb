import { useQuery } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { Table2, ChevronRight } from 'lucide-react';
import api from '../lib/api';

interface SchemasData {
  schemas: string[];
}

interface TablesData {
  tables: string[];
  count: number;
}

function SchemaSection({ schema }: { schema: string }) {
  const { data, isLoading } = useQuery<TablesData>({
    queryKey: ['schemas', schema, 'tables'],
    queryFn: () => api.get(`/schemas/${schema}/tables`).then((r) => r.data),
  });

  return (
    <div className="bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 overflow-hidden">
      <div className="px-5 py-4 border-b border-gray-200 dark:border-gray-700 flex items-center justify-between">
        <div>
          <h2 className="font-semibold text-gray-900 dark:text-gray-100">{schema}</h2>
          <p className="text-xs text-gray-400 dark:text-gray-500 mt-0.5">schema</p>
        </div>
        {!isLoading && (
          <span className="text-xs bg-gray-100 dark:bg-gray-700 text-gray-600 dark:text-gray-400 px-2 py-1 rounded-full font-medium">
            {data?.count ?? 0} tables
          </span>
        )}
      </div>

      {isLoading ? (
        <div className="px-5 py-4 text-sm text-gray-400 dark:text-gray-500">Loading tables...</div>
      ) : !data || data.tables.length === 0 ? (
        <div className="px-5 py-4 text-sm text-gray-400 dark:text-gray-500">No tables in this schema.</div>
      ) : (
        <ul className="divide-y divide-gray-100 dark:divide-gray-700">
          {data.tables.map((table) => (
            <li key={table}>
              <Link
                to={`/schemas/${schema}/tables/${table}`}
                className="flex items-center gap-3 px-5 py-3 hover:bg-gray-50 dark:hover:bg-gray-700/50 transition-colors"
              >
                <Table2 size={15} className="text-gray-400 dark:text-gray-500 flex-shrink-0" />
                <span className="flex-1 text-sm font-mono text-gray-800 dark:text-gray-200">{table}</span>
                <ChevronRight size={14} className="text-gray-300 dark:text-gray-600" />
              </Link>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export default function SchemasPage() {
  const { data, isLoading, error } = useQuery<SchemasData>({
    queryKey: ['schemas'],
    queryFn: () => api.get('/schemas').then((r) => r.data),
  });

  if (isLoading) {
    return <div className="text-gray-500 dark:text-gray-400">Loading schemas...</div>;
  }

  if (error) {
    return (
      <div className="bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-700 rounded-xl p-4 text-red-700 dark:text-red-400">
        Failed to load schemas.
      </div>
    );
  }

  const schemas = (data?.schemas ?? []).filter((s) => s === 'public');

  return (
    <div>
      <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100 mb-6">Schemas & Tables</h1>
      <div className="space-y-4">
        {schemas.map((schema) => (
          <SchemaSection key={schema} schema={schema} />
        ))}
      </div>
    </div>
  );
}
