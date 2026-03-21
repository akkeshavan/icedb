import { useState, useRef, useCallback } from 'react';
import { Play, Trash2 } from 'lucide-react';
import api from '../lib/api';

interface QueryResult {
  columns: string[];
  rows: unknown[][];
  rows_affected: number;
  command: string;
  error: string | null;
}

export default function QueryConsolePage() {
  const [sql, setSql] = useState('SELECT * FROM pg_authid;');
  const [result, setResult] = useState<QueryResult | null>(null);
  const [loading, setLoading] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const runQuery = useCallback(async () => {
    if (!sql.trim()) return;
    setLoading(true);
    try {
      const res = await api.post('/query', { sql });
      setResult(res.data);
    } catch (err: unknown) {
      setResult({
        columns: [],
        rows: [],
        rows_affected: 0,
        command: '',
        error: String((err as { message?: string })?.message ?? 'Request failed'),
      });
    } finally {
      setLoading(false);
    }
  }, [sql]);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
      e.preventDefault();
      runQuery();
    }
  };

  const clearAll = () => {
    setSql('');
    setResult(null);
    textareaRef.current?.focus();
  };

  return (
    <div className="flex flex-col h-full gap-4">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100">Query Console</h1>
        <span className="text-xs text-gray-400 dark:text-gray-500">Ctrl+Enter to run</span>
      </div>

      {/* SQL editor */}
      <div className="bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 overflow-hidden">
        <div className="flex items-center justify-between px-4 py-2 border-b border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-900/50">
          <span className="text-xs font-medium text-gray-500 dark:text-gray-400">SQL</span>
          <div className="flex gap-2">
            <button
              onClick={clearAll}
              className="flex items-center gap-1 px-2 py-1 text-xs text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200 rounded transition-colors"
            >
              <Trash2 size={12} />
              Clear
            </button>
            <button
              onClick={runQuery}
              disabled={loading || !sql.trim()}
              className="flex items-center gap-1 px-3 py-1 text-xs font-medium text-white bg-blue-600 hover:bg-blue-700 disabled:bg-blue-400 rounded transition-colors"
            >
              <Play size={12} />
              {loading ? 'Running...' : 'Run'}
            </button>
          </div>
        </div>
        <textarea
          ref={textareaRef}
          value={sql}
          onChange={(e) => setSql(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Enter SQL here..."
          rows={8}
          className="w-full px-4 py-3 text-sm font-mono text-gray-900 dark:text-gray-100 bg-white dark:bg-gray-800 focus:outline-none resize-y"
          spellCheck={false}
        />
      </div>

      {/* Results */}
      {result && (
        <div className="flex-1 overflow-auto">
          {result.error ? (
            <div className="bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-700 rounded-xl p-4">
              <div className="text-xs font-medium text-red-500 dark:text-red-400 mb-1">ERROR</div>
              <pre className="text-sm font-mono text-red-700 dark:text-red-300 whitespace-pre-wrap">
                {result.error}
              </pre>
            </div>
          ) : result.columns.length === 0 ? (
            <div className="bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 p-4">
              <div className="text-sm text-gray-600 dark:text-gray-400">
                Command: <span className="font-semibold text-gray-900 dark:text-gray-100">{result.command}</span>
                {result.rows_affected > 0 && (
                  <span className="ml-2 text-gray-500 dark:text-gray-400">
                    ({result.rows_affected} row{result.rows_affected !== 1 ? 's' : ''} affected)
                  </span>
                )}
              </div>
            </div>
          ) : (
            <div className="bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 overflow-hidden">
              <div className="px-4 py-2 border-b border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-900/50 flex items-center justify-between">
                <span className="text-xs font-medium text-gray-500 dark:text-gray-400">
                  {result.command}
                </span>
                <span className="text-xs text-gray-400 dark:text-gray-500">
                  {result.rows.length} row{result.rows.length !== 1 ? 's' : ''}
                </span>
              </div>
              <div className="overflow-auto max-h-96">
                <table className="w-full text-sm">
                  <thead className="sticky top-0 bg-gray-50 dark:bg-gray-900 border-b border-gray-200 dark:border-gray-700">
                    <tr>
                      {result.columns.map((col) => (
                        <th
                          key={col}
                          className="px-4 py-2 text-left text-xs font-medium text-gray-600 dark:text-gray-400 whitespace-nowrap"
                        >
                          {col}
                        </th>
                      ))}
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-gray-100 dark:divide-gray-700">
                    {result.rows.map((row, ri) => (
                      <tr key={ri} className="hover:bg-gray-50 dark:hover:bg-gray-700/30">
                        {(row as unknown[]).map((cell, ci) => (
                          <td
                            key={ci}
                            className="px-4 py-2 font-mono text-xs text-gray-800 dark:text-gray-200 whitespace-nowrap max-w-xs overflow-hidden text-ellipsis"
                          >
                            {cell === null ? (
                              <span className="text-gray-300 dark:text-gray-600">NULL</span>
                            ) : typeof cell === 'boolean' ? (
                              String(cell)
                            ) : (
                              String(cell)
                            )}
                          </td>
                        ))}
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
