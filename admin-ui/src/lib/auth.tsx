import React, { createContext, useContext, useState } from 'react';

interface AuthContextType {
  token: string | null;
  setToken: (token: string) => void;
  logout: () => void;
}

const AuthContext = createContext<AuthContextType | null>(null);

export function AuthProvider({ children }: { children: React.ReactNode }) {
  const [token, setTokenState] = useState<string | null>(
    () => localStorage.getItem('icedb_admin_token')
  );

  const setToken = (newToken: string) => {
    localStorage.setItem('icedb_admin_token', newToken);
    setTokenState(newToken);
  };

  const logout = () => {
    localStorage.removeItem('icedb_admin_token');
    setTokenState(null);
  };

  return (
    <AuthContext.Provider value={{ token, setToken, logout }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth(): AuthContextType {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error('useAuth must be used inside AuthProvider');
  return ctx;
}
