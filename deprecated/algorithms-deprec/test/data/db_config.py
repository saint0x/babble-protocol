"""
Test Database Configuration

This module provides database configuration for testing.
"""

import sqlite3
from pathlib import Path

class TestDatabaseConfig:
    """Test database configuration"""
    
    def __init__(self):
        self.db_path = Path(__file__).parent / 'test.db'
        self.connection = None
    
    def get_connection(self):
        """Get SQLite database connection"""
        if not self.connection:
            self.connection = sqlite3.connect(self.db_path)
        return self.connection
    
    def close(self):
        """Close database connection"""
        if self.connection:
            self.connection.close()
            self.connection = None 