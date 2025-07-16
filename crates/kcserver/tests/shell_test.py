#!/usr/bin/env python3
"""
Shell environment test script for Kallichore run_in_shell functionality.

This script inspects the current Python process environment to determine
if it's running in a shell context or directly.
"""

import os
import sys
import json
import subprocess
import shutil


def get_shell_indicators():
    """Get various indicators about the shell environment."""
    indicators = {}
    
    # Basic environment variables
    indicators['SHELL'] = os.environ.get('SHELL', 'not_set')
    indicators['PWD'] = os.environ.get('PWD', 'not_set')
    indicators['HOME'] = os.environ.get('HOME', 'not_set')
    
    # Check if common shell commands are available
    try:
        echo_path = shutil.which('echo')
        indicators['which_echo'] = echo_path if echo_path else 'not_found'
    except Exception:
        indicators['which_echo'] = 'error'
    
    return indicators


def get_process_info():
    """Get information about the current process and its ancestors."""
    try:
        # Get current process info
        pid = os.getpid()
        
        # Try to get process tree using ps
        result = subprocess.run(
            ['ps', '-o', 'pid,ppid,comm,args', '-p', str(pid)],
            capture_output=True,
            text=True,
            timeout=5
        )
        
        if result.returncode == 0:
            lines = result.stdout.strip().split('\n')
            if len(lines) >= 2:  # Header + at least one process line
                # Parse the process line (skip header)
                process_line = lines[1].strip()
                parts = process_line.split(None, 3)  # Split into at most 4 parts
                if len(parts) >= 4:
                    return {
                        'pid': parts[0],
                        'ppid': parts[1],
                        'comm': parts[2],
                        'args': parts[3]
                    }
    except Exception as e:
        return {'error': str(e)}
    
    return {'error': 'Could not get process info'}


def get_process_ancestors(max_depth=10):
    """Get process ancestry to look for shell processes."""
    ancestors = []
    current_pid = os.getpid()
    
    try:
        for depth in range(max_depth):
            # Get info for current PID
            result = subprocess.run(
                ['ps', '-o', 'ppid,comm,args', '-p', str(current_pid)],
                capture_output=True,
                text=True,
                timeout=2
            )
            
            if result.returncode != 0:
                break
                
            lines = result.stdout.strip().split('\n')
            if len(lines) < 2:  # Need header + data
                break
                
            # Parse the process line (skip header)
            process_line = lines[1].strip()
            parts = process_line.split(None, 2)  # Split into at most 3 parts
            if len(parts) < 3:
                break
                
            ppid, comm, args = parts[0], parts[1], parts[2]
            ancestors.append({
                'pid': current_pid,
                'comm': comm,
                'args': args
            })
            
            # Move to parent
            if ppid == '1' or ppid == '0':  # Reached init or kernel
                break
                
            current_pid = int(ppid)
            
    except Exception as e:
        ancestors.append({'error': str(e)})
    
    return ancestors


def analyze_shell_context(test_marker, expected_mode):
    """Analyze the current shell context and return results."""
    results = {
        'test_marker': test_marker,
        'expected_mode': expected_mode,
        'shell_indicators': get_shell_indicators(),
        'process_info': get_process_info(),
        'ancestors': get_process_ancestors(),
        'analysis': {}
    }
    
    # Analyze shell indicators
    shell_count = 0
    if results['shell_indicators']['SHELL'] != 'not_set':
        shell_count += 1
    if results['shell_indicators']['PWD'] != 'not_set':
        shell_count += 1
    if results['shell_indicators']['HOME'] != 'not_set':
        shell_count += 1
    if results['shell_indicators']['which_echo'] not in ['not_found', 'error']:
        shell_count += 1
    
    results['analysis']['shell_indicator_count'] = shell_count
    
    # Look for shell processes in ancestry
    shell_ancestors = []
    for ancestor in results['ancestors']:
        if 'comm' in ancestor:
            comm = ancestor['comm'].lower()
            if any(shell in comm for shell in ['bash', 'zsh', 'sh', 'fish', 'csh', 'tcsh']):
                shell_ancestors.append(ancestor)
    
    results['analysis']['shell_ancestors'] = shell_ancestors
    results['analysis']['has_shell_ancestor'] = len(shell_ancestors) > 0
    
    return results


def main():
    """Main function that gets called from the test."""
    if len(sys.argv) != 3:
        print("Usage: shell_test.py <test_marker> <expected_mode>")
        sys.exit(1)
    
    test_marker = sys.argv[1]
    expected_mode = sys.argv[2]  # 'shell' or 'direct'
    
    print("=== KALLICHORE SHELL TEST START ===")
    print(f"Test marker: {test_marker}")
    print(f"Expected mode: {expected_mode}")
    
    results = analyze_shell_context(test_marker, expected_mode)
    
    # Print human-readable summary
    indicators = results['shell_indicators']
    print(f"Shell indicators: {results['analysis']['shell_indicator_count']}")
    for key, value in indicators.items():
        print(f"  - {key}={value}")
    
    # Print process info
    if 'error' not in results['process_info']:
        proc = results['process_info']
        print(f"Current process: {proc['comm']} ({proc['args']})")
    
    # Print ancestors
    ancestors = results['ancestors']
    if ancestors and 'error' not in ancestors[0]:
        for i, ancestor in enumerate(ancestors):
            if 'comm' in ancestor:
                print(f"  Ancestor {i}: {ancestor['comm']} ({ancestor['args']})")
    
    # Print shell analysis
    if results['analysis']['shell_ancestors']:
        for shell in results['analysis']['shell_ancestors']:
            print(f"  Found shell ancestor: {shell['comm']}")
    
    print("=== KALLICHORE SHELL TEST END ===")
    
    # Return the full results as JSON for programmatic access
    # (This would be captured by the test if needed)
    return results


if __name__ == '__main__':
    main()
