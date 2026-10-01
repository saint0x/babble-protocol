"""
Test Database Initialization

This script initializes the SQLite database for testing.
"""

import os
import sqlite3
from pathlib import Path

def init_test_db():
    """Initialize test database with required tables"""
    db_path = Path(__file__).parent / 'test.db'
    
    # Remove existing database
    if db_path.exists():
        os.remove(db_path)
    
    # Create new database
    conn = sqlite3.connect(db_path)
    cursor = conn.cursor()
    
    # Create algorithm_errors table
    cursor.execute("""
    CREATE TABLE algorithm_errors (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        algorithm_name TEXT NOT NULL,
        error_type TEXT NOT NULL,
        error_message TEXT NOT NULL,
        context TEXT,
        timestamp REAL NOT NULL
    )
    """)
    
    # Create algorithm_metrics table
    cursor.execute("""
    CREATE TABLE algorithm_metrics (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        algorithm_name TEXT NOT NULL,
        metric_name TEXT NOT NULL,
        metric_value REAL NOT NULL,
        timestamp REAL NOT NULL
    )
    """)
    
    # Create content_cache table
    cursor.execute("""
    CREATE TABLE content_cache (
        content_id TEXT PRIMARY KEY,
        content_type TEXT NOT NULL,
        content TEXT NOT NULL,
        metadata TEXT,
        timestamp REAL NOT NULL
    )
    """)
    
    # Create user_profiles table
    cursor.execute("""
    CREATE TABLE user_profiles (
        user_id TEXT PRIMARY KEY,
        profile_data TEXT NOT NULL,
        last_updated REAL NOT NULL
    )
    """)
    
    # Create engagement_logs table
    cursor.execute("""
    CREATE TABLE engagement_logs (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        user_id TEXT NOT NULL,
        content_id TEXT NOT NULL,
        engagement_type TEXT NOT NULL,
        engagement_data TEXT,
        timestamp REAL NOT NULL
    )
    """)
    
    conn.commit()
    conn.close()

if __name__ == "__main__":
    init_test_db() 