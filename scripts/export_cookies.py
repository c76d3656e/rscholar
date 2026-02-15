#!/usr/bin/env python3
"""
Google Scholar Cookie Exporter

This script exports Google Scholar cookies from your browser 
and saves them in a format compatible with Rscholar.

Usage:
    python export_cookies.py [browser]
    
Browser options: chrome, edge, firefox (default: chrome)

Requirements:
    pip install browser-cookie3
"""

import json
import sys
import os

def get_cookie_path():
    """Get the default cookie file path for Rscholar"""
    home = os.path.expanduser("~")
    return os.path.join(home, ".gscholar_cookies.json")

def export_cookies(browser_name="chrome"):
    """Export Google Scholar cookies from the specified browser"""
    try:
        import browser_cookie3
    except ImportError:
        print("Error: browser_cookie3 not installed.")
        print("Install it with: pip install browser-cookie3")
        sys.exit(1)
    
    # Get browser cookie jar
    browser_funcs = {
        "chrome": browser_cookie3.chrome,
        "edge": browser_cookie3.edge,
        "firefox": browser_cookie3.firefox,
        "opera": browser_cookie3.opera,
        "brave": browser_cookie3.brave,
    }
    
    if browser_name not in browser_funcs:
        print(f"Unknown browser: {browser_name}")
        print(f"Supported browsers: {', '.join(browser_funcs.keys())}")
        sys.exit(1)
    
    print(f"Extracting cookies from {browser_name}...")
    
    try:
        cj = browser_funcs[browser_name](domain_name=".google.com")
    except Exception as e:
        print(f"Error accessing {browser_name} cookies: {e}")
        print("Make sure the browser is closed and try again.")
        sys.exit(1)
    
    # Convert to Rscholar format
    cookies = []
    for cookie in cj:
        if "google" in cookie.domain:
            cookies.append({
                "name": cookie.name,
                "value": cookie.value,
                "domain": cookie.domain,
                "path": cookie.path or "/",
                "secure": cookie.secure,
                "http_only": bool(cookie.has_nonstandard_attr("HttpOnly")),
                "expires": cookie.expires
            })
    
    if not cookies:
        print("No Google cookies found!")
        print("Please visit https://scholar.google.com first and complete any CAPTCHA.")
        sys.exit(1)
    
    # Save to file
    cookie_path = get_cookie_path()
    with open(cookie_path, "w", encoding="utf-8") as f:
        json.dump(cookies, f, indent=2)
    
    print(f"Exported {len(cookies)} cookies to: {cookie_path}")
    print()
    print("Cookie names exported:")
    for c in cookies[:10]:  # Show first 10
        print(f"  - {c['name']}")
    if len(cookies) > 10:
        print(f"  ... and {len(cookies) - 10} more")
    
    return cookies

def main():
    browser = sys.argv[1] if len(sys.argv) > 1 else "chrome"
    export_cookies(browser)
    print()
    print("Done! You can now run Rscholar with Google Scholar source.")

if __name__ == "__main__":
    main()
