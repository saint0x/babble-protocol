"""
Initialize SQLite database for testing
"""
import sqlite3
from pathlib import Path

def init_db():
    """Initialize test database with required tables"""
    db_path = Path(__file__).parent / "test.db"
    conn = sqlite3.connect(db_path)
    cursor = conn.cursor()

    # Create algorithm cache table
    cursor.execute("""
    CREATE TABLE IF NOT EXISTS algorithm_cache (
        key TEXT PRIMARY KEY,
        value TEXT,
        expiry FLOAT
    )
    """)

    # Create algorithm metrics table
    cursor.execute("""
    CREATE TABLE IF NOT EXISTS algorithm_metrics (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        algorithm_name TEXT,
        metric_name TEXT,
        value FLOAT,
        timestamp FLOAT
    )
    """)

    # Create algorithm errors table
    cursor.execute("""
    CREATE TABLE IF NOT EXISTS algorithm_errors (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        algorithm_name TEXT,
        error_type TEXT,
        error_message TEXT,
        context TEXT,
        timestamp FLOAT
    )
    """)

    # Create users table
    cursor.execute("""
    CREATE TABLE IF NOT EXISTS users (
        user_id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        bio TEXT,
        created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    )
    """)

    # Create user interests table
    cursor.execute("""
    CREATE TABLE IF NOT EXISTS user_interests (
        user_id TEXT,
        interest TEXT,
        PRIMARY KEY (user_id, interest),
        FOREIGN KEY (user_id) REFERENCES users(user_id)
    )
    """)

    # Create user expertise table
    cursor.execute("""
    CREATE TABLE IF NOT EXISTS user_expertise (
        user_id TEXT,
        expertise TEXT,
        PRIMARY KEY (user_id, expertise),
        FOREIGN KEY (user_id) REFERENCES users(user_id)
    )
    """)

    # Create user engagement table
    cursor.execute("""
    CREATE TABLE IF NOT EXISTS user_engagement (
        user_id TEXT,
        metric TEXT,
        value FLOAT,
        PRIMARY KEY (user_id, metric),
        FOREIGN KEY (user_id) REFERENCES users(user_id)
    )
    """)

    # Create content table
    cursor.execute("""
    CREATE TABLE IF NOT EXISTS content (
        content_id TEXT PRIMARY KEY,
        title TEXT NOT NULL,
        text TEXT NOT NULL,
        created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    )
    """)

    # Create content tags table
    cursor.execute("""
    CREATE TABLE IF NOT EXISTS content_tags (
        content_id TEXT,
        tag TEXT,
        PRIMARY KEY (content_id, tag),
        FOREIGN KEY (content_id) REFERENCES content(content_id)
    )
    """)

    # Create content metadata table
    cursor.execute("""
    CREATE TABLE IF NOT EXISTS content_metadata (
        content_id TEXT PRIMARY KEY,
        complexity_level FLOAT,
        FOREIGN KEY (content_id) REFERENCES content(content_id)
    )
    """)

    # Create content recommendations table
    cursor.execute("""
    CREATE TABLE IF NOT EXISTS content_recommendations (
        content_id TEXT,
        user_id TEXT,
        score FLOAT,
        PRIMARY KEY (content_id, user_id),
        FOREIGN KEY (content_id) REFERENCES content(content_id),
        FOREIGN KEY (user_id) REFERENCES users(user_id)
    )
    """)

    # Create user feedback table
    cursor.execute("""
    CREATE TABLE IF NOT EXISTS user_feedback (
        feedback_id TEXT PRIMARY KEY,
        user_id TEXT,
        content_id TEXT,
        feedback_type TEXT,
        feedback_value FLOAT,
        created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
        FOREIGN KEY (user_id) REFERENCES users(user_id),
        FOREIGN KEY (content_id) REFERENCES content(content_id)
    )
    """)

    conn.commit()
    conn.close()

if __name__ == "__main__":
    init_db() 