#![deny(clippy::all)]

//! mysqldump-cli — OurOS mysqldump CLI
//!
//! Single personality: `mysqldump`

use std::env;
use std::process;

fn run_mysqldump(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mysqldump [OPTIONS] DATABASE [TABLES]");
        println!();
        println!("mysqldump — MySQL database backup tool (OurOS).");
        println!();
        println!("Options:");
        println!("  -h, --host HOST        Server host");
        println!("  -P, --port PORT        Port");
        println!("  -u, --user USER        Username");
        println!("  -p, --password[=PASS]  Password");
        println!("  --all-databases        Dump all databases");
        println!("  --single-transaction   Consistent snapshot");
        println!("  --routines             Include stored routines");
        println!("  --triggers             Include triggers");
        println!("  --add-drop-table       Add DROP TABLE");
        println!("  --no-data              Schema only");
        println!("  --no-create-info       Data only");
        println!("  --compress             Compress communication");
        println!("  -r, --result-file FILE Output file");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("mysqldump  Ver 8.0.35 (OurOS)");
        return 0;
    }

    let all_db = args.iter().any(|a| a == "--all-databases" || a == "-A");
    let no_data = args.iter().any(|a| a == "--no-data" || a == "-d");

    let db = args.iter().filter(|a| !a.starts_with('-'))
        .next().map(|s| s.as_str());

    println!("-- MySQL dump 8.0.35 (OurOS)");
    println!("-- Host: localhost    Database: {}", db.unwrap_or("(all)"));
    println!("-- Server version  8.0.35");
    println!();
    println!("/*!40101 SET @OLD_CHARACTER_SET_CLIENT=@@CHARACTER_SET_CLIENT */;");
    println!("/*!40101 SET NAMES utf8mb4 */;");
    println!();

    if all_db {
        println!("-- Current Database: `mydb`");
        println!("CREATE DATABASE /*!32312 IF NOT EXISTS*/ `mydb`;");
        println!("USE `mydb`;");
        println!();
    }

    println!("DROP TABLE IF EXISTS `users`;");
    println!("CREATE TABLE `users` (");
    println!("  `id` int NOT NULL AUTO_INCREMENT,");
    println!("  `name` varchar(255) NOT NULL,");
    println!("  `email` varchar(255) DEFAULT NULL,");
    println!("  PRIMARY KEY (`id`),");
    println!("  UNIQUE KEY `email` (`email`)");
    println!(") ENGINE=InnoDB AUTO_INCREMENT=4 DEFAULT CHARSET=utf8mb4;");

    if !no_data {
        println!();
        println!("INSERT INTO `users` VALUES (1,'Alice','alice@example.com'),");
        println!("(2,'Bob','bob@example.com'),(3,'Charlie','charlie@example.com');");
    }

    println!();
    println!("-- Dump completed on 2024-01-15 12:00:00");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mysqldump(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_mysqldump};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_mysqldump(vec!["--help".to_string()]), 0);
        assert_eq!(run_mysqldump(vec!["-h".to_string()]), 0);
        assert_eq!(run_mysqldump(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_mysqldump(vec![]), 0);
    }
}
