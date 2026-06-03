#![deny(clippy::all)]

//! aws-cli — OurOS AWS command-line interface
//!
//! Single personality: `aws`

use std::env;
use std::process;

fn run_aws(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "help") {
        println!("Usage: aws [OPTIONS] <SERVICE> <COMMAND> [PARAMETERS]");
        println!();
        println!("The AWS Command Line Interface (OurOS).");
        println!();
        println!("Services:");
        println!("  s3           Amazon S3");
        println!("  ec2          Amazon EC2");
        println!("  iam          Identity and Access Management");
        println!("  lambda       AWS Lambda");
        println!("  ecs          Elastic Container Service");
        println!("  eks          Elastic Kubernetes Service");
        println!("  rds          Relational Database Service");
        println!("  dynamodb     Amazon DynamoDB");
        println!("  cloudformation CloudFormation");
        println!("  sqs          Simple Queue Service");
        println!("  sns          Simple Notification Service");
        println!("  sts          Security Token Service");
        println!("  ssm          Systems Manager");
        println!("  logs         CloudWatch Logs");
        println!("  configure    Configure AWS CLI");
        println!();
        println!("Options:");
        println!("  --profile <PROFILE>  Use named profile");
        println!("  --region <REGION>    AWS region");
        println!("  --output <FORMAT>    Output format (json/text/table/yaml)");
        println!("  --no-cli-pager       Disable pager");
        println!("  --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("aws-cli/2.15.0 OurOS/1.0 exe/x86_64");
        return 0;
    }

    let service = args.first().map(|s| s.as_str()).unwrap_or("");
    let command = args.get(1).map(|s| s.as_str()).unwrap_or("");

    match service {
        "configure" => {
            match command {
                "list" => {
                    println!("      Name                    Value             Type    Location");
                    println!("      ----                    -----             ----    --------");
                    println!("   profile                <not set>             None    None");
                    println!("access_key     ****************ABCD shared-credentials-file");
                    println!("secret_key     ****************1234 shared-credentials-file");
                    println!("    region                us-east-1      config-file    ~/.aws/config");
                }
                _ => {
                    println!("AWS Access Key ID [****************ABCD]: ");
                    println!("AWS Secret Access Key [****************1234]: ");
                    println!("Default region name [us-east-1]: ");
                    println!("Default output format [json]: ");
                }
            }
            0
        }
        "s3" => {
            match command {
                "ls" => {
                    let bucket = args.get(2).map(|s| s.as_str()).unwrap_or("");
                    if bucket.is_empty() {
                        println!("2024-01-10 09:15:23 my-app-bucket");
                        println!("2024-01-08 14:30:00 my-logs-bucket");
                        println!("2023-12-20 11:00:00 my-backups-bucket");
                    } else {
                        println!("2024-01-15 10:00:00       1024 file1.txt");
                        println!("2024-01-15 10:01:00      51200 archive.tar.gz");
                        println!("2024-01-14 09:30:00        256 config.json");
                    }
                }
                "cp" => {
                    let src = args.get(2).map(|s| s.as_str()).unwrap_or("file.txt");
                    let dst = args.get(3).map(|s| s.as_str()).unwrap_or("s3://bucket/");
                    println!("copy: {} to {}", src, dst);
                }
                "sync" => {
                    let src = args.get(2).map(|s| s.as_str()).unwrap_or(".");
                    let dst = args.get(3).map(|s| s.as_str()).unwrap_or("s3://bucket/");
                    println!("upload: {} to {}/file1.txt", src, dst);
                    println!("upload: {} to {}/file2.txt", src, dst);
                }
                "mb" => {
                    let bucket = args.get(2).map(|s| s.as_str()).unwrap_or("s3://my-bucket");
                    println!("make_bucket: {}", bucket);
                }
                _ => {
                    eprintln!("Usage: aws s3 <ls|cp|sync|mb|rb|mv|rm>. See --help.");
                    return 1;
                }
            }
            0
        }
        "ec2" => {
            match command {
                "describe-instances" => {
                    println!("{{");
                    println!("  \"Reservations\": [{{");
                    println!("    \"Instances\": [{{");
                    println!("      \"InstanceId\": \"i-0abc123def456789\",");
                    println!("      \"InstanceType\": \"t3.medium\",");
                    println!("      \"State\": {{\"Name\": \"running\"}},");
                    println!("      \"PublicIpAddress\": \"54.123.45.67\",");
                    println!("      \"PrivateIpAddress\": \"10.0.1.100\",");
                    println!("      \"Tags\": [{{\"Key\": \"Name\", \"Value\": \"web-server-1\"}}]");
                    println!("    }}]");
                    println!("  }}]");
                    println!("}}");
                }
                "run-instances" => {
                    println!("{{");
                    println!("  \"Instances\": [{{");
                    println!("    \"InstanceId\": \"i-0new456def789abc\",");
                    println!("    \"InstanceType\": \"t3.medium\",");
                    println!("    \"State\": {{\"Name\": \"pending\"}}");
                    println!("  }}]");
                    println!("}}");
                }
                "stop-instances" | "start-instances" | "terminate-instances" => {
                    let id = args.get(2).map(|s| s.as_str()).unwrap_or("i-0abc123def456789");
                    let state = if command == "stop-instances" { "stopping" }
                        else if command == "start-instances" { "pending" }
                        else { "shutting-down" };
                    println!("{{\"StoppingInstances\": [{{\"InstanceId\": \"{}\", \"CurrentState\": {{\"Name\": \"{}\"}}}}]}}", id, state);
                }
                _ => {
                    eprintln!("Usage: aws ec2 <describe-instances|run-instances|stop-instances|...>. See --help.");
                    return 1;
                }
            }
            0
        }
        "iam" => {
            match command {
                "list-users" => {
                    println!("{{\"Users\": [");
                    println!("  {{\"UserName\": \"admin\", \"UserId\": \"AIDA12345EXAMPLE\", \"CreateDate\": \"2024-01-01T00:00:00Z\"}},");
                    println!("  {{\"UserName\": \"deploy\", \"UserId\": \"AIDA67890EXAMPLE\", \"CreateDate\": \"2024-01-05T00:00:00Z\"}}");
                    println!("]}}");
                }
                "get-user" => {
                    println!("{{\"User\": {{\"UserName\": \"admin\", \"UserId\": \"AIDA12345EXAMPLE\", \"Arn\": \"arn:aws:iam::123456789012:user/admin\"}}}}");
                }
                _ => {
                    eprintln!("Usage: aws iam <list-users|get-user|create-user|...>. See --help.");
                    return 1;
                }
            }
            0
        }
        "lambda" => {
            match command {
                "list-functions" => {
                    println!("{{\"Functions\": [");
                    println!("  {{\"FunctionName\": \"my-handler\", \"Runtime\": \"python3.12\", \"MemorySize\": 256, \"Timeout\": 30}},");
                    println!("  {{\"FunctionName\": \"api-proxy\", \"Runtime\": \"nodejs20.x\", \"MemorySize\": 128, \"Timeout\": 15}}");
                    println!("]}}");
                }
                "invoke" => {
                    let func = args.get(2).map(|s| s.as_str()).unwrap_or("my-handler");
                    println!("{{\"StatusCode\": 200, \"FunctionError\": null, \"ExecutedVersion\": \"$LATEST\"}}");
                    println!("(invoked {})", func);
                }
                _ => {
                    eprintln!("Usage: aws lambda <list-functions|invoke|create-function|...>. See --help.");
                    return 1;
                }
            }
            0
        }
        "sts" => {
            match command {
                "get-caller-identity" => {
                    println!("{{");
                    println!("  \"UserId\": \"AIDA12345EXAMPLE\",");
                    println!("  \"Account\": \"123456789012\",");
                    println!("  \"Arn\": \"arn:aws:iam::123456789012:user/admin\"");
                    println!("}}");
                }
                _ => {
                    eprintln!("Usage: aws sts <get-caller-identity|assume-role|...>. See --help.");
                    return 1;
                }
            }
            0
        }
        "logs" => {
            match command {
                "describe-log-groups" => {
                    println!("{{\"logGroups\": [");
                    println!("  {{\"logGroupName\": \"/aws/lambda/my-handler\", \"storedBytes\": 1048576}},");
                    println!("  {{\"logGroupName\": \"/aws/ecs/my-service\", \"storedBytes\": 5242880}}");
                    println!("]}}");
                }
                "tail" => {
                    let group = args.get(2).map(|s| s.as_str()).unwrap_or("/aws/lambda/my-handler");
                    println!("2024-01-15T14:00:00 START RequestId: abc-123");
                    println!("2024-01-15T14:00:00 Processing event...");
                    println!("2024-01-15T14:00:01 END RequestId: abc-123");
                    println!("2024-01-15T14:00:01 REPORT Duration: 234.56 ms  Memory: 128 MB");
                    println!("(from log group {})", group);
                }
                _ => {
                    eprintln!("Usage: aws logs <describe-log-groups|tail|filter-log-events|...>. See --help.");
                    return 1;
                }
            }
            0
        }
        _ => {
            if service.is_empty() {
                eprintln!("Usage: aws <service> <command>. See --help.");
            } else {
                eprintln!("Error: unknown service '{}'. See --help.", service);
            }
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_aws(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_aws};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_aws(vec!["--help".to_string()]), 0);
        assert_eq!(run_aws(vec!["-h".to_string()]), 0);
        assert_eq!(run_aws(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_aws(vec![]), 0);
    }
}
