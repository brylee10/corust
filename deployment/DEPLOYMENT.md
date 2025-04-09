# `CloudWatch`
Corust uses `CloudWatch` to monitor server metrics and process status on machines. The CloudWatch agent configuration is `corust-cloudwatch.json`. This can be enabled on the machine via

```sh
sudo /opt/aws/amazon-cloudwatch-agent/bin/amazon-cloudwatch-agent-ctl -a fetch-config -m ec2 -c file:corust/deployment/corust-cloudwatch.json -s
```

The CloudWatch alerts can be configured in the AWS console following this guide: https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/using-cloudwatch-createalarm.html.